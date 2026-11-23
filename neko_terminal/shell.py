"""Run a real shell on a PTY and draw the cat only over unused cells."""

import codecs
import errno
import fcntl
import os
import pty
import re
import select
import struct
import sys
import termios
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import suppress
from functools import lru_cache
from math import ceil
from pathlib import Path

import pyte
from pyte import graphics, modes

from .animation import Frame, act, fit_scale, now, read_pane, seed_at, state, world_width
from .care import StatusLine, feed
from .poses import KEYS, clamp, special
from .scene import scene
from .terminal import cursor, emit, fit_text, raw_terminal, sgr

WIDTH = 40
SYNC_ON = b"\x1b[?2026h"
SYNC_OFF = b"\x1b[?2026l"
ALT_MODES = {47, 1047, 1049}
MARGINS = re.compile(r"\x1b\[([\d;]*)r")
PRIVATE_MODE = re.compile(r"\x1b\[\?([\d;]+)([lh])")
CELL_EDIT = re.compile(r"\x1b\[\??(\d*)([JKXP@ST])")
CONTROL = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|[\]PX^_][\s\S]*?(?:\x07|\x1b\\)|[ -/]*[0-~])")
ANSI_COLORS = {v: k for k, v in {**graphics.FG_ANSI, **graphics.FG_AIXTERM}.items()}


def log(text):
    if path := os.environ.get("NEKO_LOG"):
        with suppress(OSError), Path(path).open("a") as file:
            print(f"{now():.2f} {text}", file=file)


def winsize(fd, cols, lines):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", max(1, lines), max(1, cols), 0, 0))


@lru_cache(maxsize=256)
def attributes(char):
    attrs = ["0"]
    for color, base in ((char.fg, 3), (char.bg, 4)):
        if color in ANSI_COLORS:
            attrs.append(str(ANSI_COLORS[color] + (base - 3) * 10))
        elif len(color) == 6:
            attrs.append(f"{base}8;2;" + ";".join(str(int(color[i : i + 2], 16)) for i in (0, 2, 4)))
    for attr, code in [
        ("bold", "1"),
        ("italics", "3"),
        ("underscore", "4"),
        ("blink", "5"),
        ("reverse", "7"),
        ("strikethrough", "9"),
    ]:
        if getattr(char, attr):
            attrs.append(code)
    return "\x1b[" + ";".join(attrs) + "m"


class Display:
    """Track the shell surface independently of the actual painted overlay."""

    def __init__(self, cols, lines):
        self.primary = pyte.Screen(cols, lines)
        self.screen = self.primary
        self.stream = pyte.Stream(self.screen)
        self.decoder = codecs.getincrementaldecoder("utf-8")("surrogateescape")
        self.pending = ""
        self.erased_ranges = []
        self.bar_erased = False
        self.synchronized = False
        self.end_sync = False
        self.heat = [0.0] * cols
        self.occupied = []
        self.index()

    @property
    def alternate(self):
        return self.screen is not self.primary

    def resize(self, cols, lines):
        self.primary.resize(lines, cols)
        if self.alternate:
            self.screen.resize(lines, cols)
        self.heat = [0.0] * cols
        self.index()

    def index(self):
        # A bit per occupied column makes searching all 40-column rectangles cheap.
        scr = self.screen
        self.occupied = []
        for row in range(scr.lines):
            mask = 0
            for col, char in scr.buffer[row].items():
                if char.data != " " or char.reverse or char.bg != "default":
                    mask |= 1 << col
            self.occupied.append(mask)

    def process(self, data):
        self.erased_ranges.clear()
        self.bar_erased = False
        text = self.pending + self.decoder.decode(data)
        self.pending = ""
        output = []
        start = 0
        while start < len(text):
            esc = text.find("\x1b", start)
            end = len(text) if esc < 0 else esc
            if end > start:
                self.stream.feed(text[start:end])
                output.append(text[start:end])
            if esc < 0:
                break
            match = CONTROL.match(text, esc)
            # The generic two-byte ESC alternative can also match a CSI/OSC prefix.
            if not match or (match.end() == esc + 2 and text[esc + 1 : esc + 2] in ("[", "]", "P", "X", "^", "_")):
                self.pending = text[esc:]
                break
            token = match[0]
            start = match.end()
            toggle = PRIVATE_MODE.fullmatch(token)
            if toggle:
                params = {int(p) for p in toggle[1].split(";") if p}
                if params & ALT_MODES:
                    enable = toggle[2] == "h"
                    if enable != self.alternate:
                        self.bar_erased = True
                        self.erased_ranges.append(((0, 0), (self.screen.lines, 0)))
                        self.screen = pyte.Screen(self.primary.columns, self.primary.lines) if enable else self.primary
                        self.stream = pyte.Stream(self.screen)
            if margin := MARGINS.fullmatch(token):
                vals = margin[1].split(";")
                top = int(vals[0] or 1) or 1
                bottom = int(vals[1] or 0) if len(vals) > 1 else 0
                token = f"\x1b[{top};{min(bottom or self.screen.lines, self.screen.lines)}r"
            self.apply_control(token)
            if toggle and 2026 in params:
                self.synchronized = toggle[2] == "h"
                self.end_sync = not self.synchronized
                if self.end_sync:
                    # Commit the application's update after any replacement pixels.
                    remaining = [p for p in toggle[1].split(";") if p and int(p) != 2026]
                    token = "\x1b[?" + ";".join(remaining) + "l" if remaining else ""
            output.append(token)
            if token == "\x1bc":
                output.append(f"\x1b[1;{self.screen.lines}r")
        self.index()
        for col, h in enumerate(self.heat):
            used = sum((row >> col) & 1 for row in self.occupied)
            self.heat[col] = h * 0.97 + 0.03 * used / self.screen.lines
        return "".join(output).encode("utf-8", "surrogateescape")

    def apply_control(self, token):
        """Track physical edits that can leave blank virtual cells looking unchanged."""
        scr = self.screen
        if edit := CELL_EDIT.fullmatch(token):
            count, op = int(edit[1] or 0), edit[2]
            y, x = scr.cursor.y, scr.cursor.x
            first, last = (y, x), (y, scr.columns)
            if op in "JK":
                if count in (1, 2, 3):
                    first = (0 if op == "J" else y, 0)
                last = (y, x) if count == 1 else (scr.lines if op == "J" else y, scr.columns)
                self.bar_erased |= op == "J" and count in (0, 2, 3)
            elif op == "X":
                last = (y, x + (count or 1) - 1)
            elif op in "ST":
                # pyte implements IND/RI but has no CSI scroll-up/down dispatch.
                top, bottom = scr.margins or (0, scr.lines - 1)
                scr.cursor.y = bottom if op == "S" else top
                scroll = scr.index if op == "S" else scr.reverse_index
                for _ in range(min(bottom - top + 1, count or 1)):
                    scroll()
                scr.cursor.y = y
                return
            self.erased_ranges.append((first, last))
        elif token == "\x1bc":
            self.bar_erased = True
            self.erased_ranges.append(((0, 0), (scr.lines, 0)))
        self.stream.feed(token)

    def blank(self, x, y, w, h):
        if x < 0 or y < 0 or x + w > self.screen.columns or y + h > self.screen.lines:
            return False
        mask = ((1 << w) - 1) << x
        return not any(row & mask for row in self.occupied[y : y + h])

    def clear_of_cursor(self, y, h):
        return not y - 1 <= self.screen.cursor.y <= y + h

    def spots(self, h):
        options = []
        for x in range(0, self.screen.columns - WIDTH + 1, 2):
            usage = sum(self.heat[x : x + WIDTH]) * 10
            for y in range(self.screen.lines - h + 1):
                if self.clear_of_cursor(y, h) and self.blank(x, y, WIDTH, h):
                    score = -usage + x * 0.05 + y * 0.02 + 3 * (y + h <= self.screen.cursor.y)
                    options.append((score, x, y))
        options.sort(reverse=True)
        return options

    def cursor_guard(self):
        """Controls for drawing on the physical terminal around the shell cursor."""
        scr = self.screen
        origin = modes.DECOM in scr.mode
        row = scr.cursor.y - (scr.margins.top if origin and scr.margins else 0)
        col = scr.cursor.x
        if col == scr.columns:
            # CUP alone loses the delayed autowrap at the right margin. Replay
            # the last glyph so the shell's next character still wraps.
            col -= 1
            if not scr.buffer[scr.cursor.y][col].data and col:
                col -= 1
            char = scr.buffer[scr.cursor.y][col]
            restore = cursor(row, col) + attributes(char) + (char.data or " ")
        else:
            restore = cursor(row, col)
        before = "\x1b[?25l" + ("\x1b[?6l" if origin else "")
        after = ("\x1b[?6h" if origin else "") + restore + attributes(scr.cursor.attrs)
        return before, after + ("" if scr.cursor.hidden else "\x1b[?25h")

    def decorate(self, text):
        if not text:
            return ""
        before, after = self.cursor_guard()
        return before + text + after

    def packet(self, erased, shown, update):
        """Present the shell bytes and any overlay changes in a single write."""
        if erased or update:
            shown = SYNC_ON + erased + shown + self.decorate(update).encode()
            self.end_sync = not self.synchronized
        if self.end_sync:
            shown += SYNC_OFF
            self.end_sync = False
        return shown


class Overlay:
    def __init__(self):
        self.pos = None
        self.travel = None
        self.drawn = {}
        self.wander_at = now() + 8

    def paint(self, want, display):
        output = []
        for r, c in self.drawn.keys() - want.keys():
            char = display.screen.buffer[r][c]
            output.extend((cursor(r, c), attributes(char), char.data or " "))
        last = None
        for (r, c), cell in sorted(want.items()):
            if self.drawn.get((r, c)) == cell:
                continue
            ch, fg, bg = cell
            output.append(cursor(r, c))
            if last != (fg, bg):
                output.append(sgr(fg, bg))
                last = fg, bg
            output.append(chr(ch))
        self.drawn = want
        return "".join(output)

    def forward(self, display, data):
        """Return the old surface restoration (if needed) and the completed shell output."""
        if not self.drawn:
            return b"", display.process(data)
        old = display.screen
        before, after = display.cursor_guard()
        rows = {r: old.buffer[r] for r, _ in self.drawn}
        cells = {(r, c): rows[r][c] for r, c in self.drawn}
        shown = display.process(data)
        new = display.screen
        # A scroll moves row objects even if every underlying cell was blank.
        moved = old is not new or any(line is not new.buffer[r] for r, line in rows.items())
        overwritten = any(new.buffer[r][c] is not char for (r, c), char in cells.items())
        cleared = any(first <= pos <= last for first, last in display.erased_ranges for pos in cells)
        if not (moved or overwritten or cleared):
            return b"", shown
        erase = "".join(cursor(r, c) + attributes(char) + (char.data or " ") for (r, c), char in cells.items())
        self.drawn.clear()
        if moved:
            self.pos = self.travel = None
        return (before + erase + after).encode(), shown

    def move(self, display, dest, frame, k, h):
        start = self.pos
        self.pos = dest
        if not start or not dest or start == dest:
            return
        col, row = start[0] + frame.x * k, start[1]
        goal, y = dest[0] + frame.x * k, dest[1]
        half = ceil(17 * k)
        cols = display.screen.columns

        def band(a, b, top, bottom):
            left = max(0, round(min(a, b)) - half)
            right = min(cols - 1, round(max(a, b)) + half)
            return left <= right and display.blank(left, top, right - left + 1, bottom - top)

        via = goal
        edge = False
        if row != y:
            choices = sorted(range(cols), key=lambda x: abs(x - col) + abs(x - goal))
            via = next(
                (
                    x
                    for x in choices
                    if band(x, x, min(row, y), max(row, y) + h)
                    and band(col, x, row, row + h)
                    and band(x, goal, y, y + h)
                ),
                None,
            )
            if via is None:
                edges = sorted((-half - 1, cols + half), key=lambda x: abs(x - col) + abs(x - goal))
                via = next((x for x in edges if band(col, x, row, row + h) and band(x, goal, y, y + h)), None)
                edge = True
        if via is not None:
            self.travel = self.steps(col, row, goal, y, via, edge, k, cols)
            log(f"go {start} -> {dest} via {via}")
        else:
            log(f"no path {start} -> {dest}")

    @staticmethod
    def steps(col, row, goal, y, via, edge, k, cols):
        distance, hop, facing = 0.0, 0, 1.0
        while True:
            aim = via if row != y else goal
            if abs(aim - col) >= 0.5 and hop in (0, 1000):
                delta = clamp(aim - col, -1, 1)
                col += delta
                distance += abs(delta) / k
                facing = 1 if delta > 0 else -1
                node, anim = "Stand", "Walk"
            elif row != y and edge:
                row, hop = y, 1000
                node, anim = "Stand", "Walk"
            elif row != y:
                hop += 1
                node, anim = "Crouch", "Still"
                if hop > 2:
                    row += 1 if y > row else -1
                    node = "Air"
            elif 0 < hop < 1000:
                hop = 1000
                node, anim = "Crouch", "Still"
            else:
                return
            bx = clamp(round(col - WIDTH / 2), 0, cols - WIDTH)
            pose = special(KEYS[node], anim, now(), distance)
            yield Frame(pose, pose, node, (col - bx) / k, facing, anim, ""), bx, row

    def frame(self, display, path, t):
        scr = display.screen
        if display.alternate or scr.columns < WIDTH:
            self.pos = self.travel = None
            return self.paint({}, display)
        k = fit_scale(WIDTH, clamp(scr.lines / 33, 0.4, 1))
        width = world_width(WIDTH, k)
        h = ceil(11 * k)
        fr = state(read_pane(path, width), t, width)
        if self.travel is None:
            vacant = self.pos and display.blank(*self.pos, WIDTH, h)
            safe = vacant and display.clear_of_cursor(self.pos[1], h)
            if not safe or t >= self.wander_at:
                choices = display.spots(h)
                dest = None
                if choices:
                    if safe:
                        seed = seed_at(t)
                        far = [c for c in choices[:40] if abs(c[1] - self.pos[0]) + abs(c[2] - self.pos[1]) >= 4]
                        dest = far[(seed >> 8) % len(far)][1:] if far else self.pos
                    elif self.pos:
                        dest = max(
                            choices, key=lambda c: c[0] - 0.05 * (abs(c[1] - self.pos[0]) + abs(c[2] - self.pos[1]))
                        )[1:]
                    else:
                        dest = choices[0][1:]
                if vacant:
                    self.move(display, dest, fr, k, h)
                else:
                    if self.pos != dest:
                        log(f"place {dest}")
                    self.pos = dest
                self.wander_at = t + (12 + seed_at(t) % 25 if safe else 8)
        step = next(self.travel, None) if self.travel else None
        if step:
            fr, x, y = step
        else:
            self.travel = None
            if not self.pos:
                return self.paint({}, display)
            x, y = self.pos
            if not display.blank(x, y, WIDTH, h):
                self.pos = None
                return self.paint({}, display)
        cells = scene(fr, WIDTH, h * 2, k, t)
        want = {
            (y + r, x + c): tuple(cell)
            for r, row in enumerate(cells.tolist())
            for c, cell in enumerate(row)
            if cell[0] != 32
        }
        if step and any(not display.blank(c, r, 1, 1) for r, c in want):
            self.travel = None
            log("path blocked")
            want = {}
        return self.paint(want, display)


def session(pid, master, cols, rows, name, path):
    display = Display(cols, max(1, rows - 1))
    cat = Overlay()
    status = StatusLine()
    armed = dozing = False
    busy_since = None
    pending_feed = None
    message, last_bar = "", None
    until = next_frame = next_fg = 0.0
    with raw_terminal(), ThreadPoolExecutor(max_workers=1) as worker:
        try:
            emit(f"\x1b[0m\x1b[2J\x1b[1;{display.screen.lines}r\x1b[H")
            while True:
                ready = select.select([0, master], [], [], 0.04)[0]
                t = time.monotonic()
                new_size = os.get_terminal_size(0)
                if new_size != (cols, rows):
                    emit(display.decorate(cat.paint({}, display)))
                    cols, rows = new_size
                    inner = max(1, rows - 1)
                    winsize(master, cols, inner)
                    display.resize(cols, inner)
                    cat = Overlay()
                    emit(f"\x1b[1;{inner}r" + cursor(display.screen.cursor.y, display.screen.cursor.x))
                    last_bar = None
                if 0 in ready:
                    data = os.read(0, 4096)
                    if not data:
                        break
                    send = bytearray()
                    for b in data:
                        if armed:
                            armed = False
                            if 49 <= b <= 51 and pending_feed is None:
                                food = ("dry", "wet", "treat")[b - 49]
                                pending_feed = worker.submit(feed, food, path)
                            elif b == 29:
                                send.append(b)
                        elif b == 29:
                            armed = True
                        else:
                            send.append(b)
                    while send:
                        del send[: os.write(master, send)]
                erased = shown = b""
                if master in ready:
                    try:
                        data = os.read(master, 65536)
                    except OSError as exc:
                        if exc.errno != errno.EIO:
                            raise
                        break
                    if not data:
                        break
                    erased, shown = cat.forward(display, data)
                    if len(shown) >= 4096:
                        next_frame = t + 0.15
                    elif erased:
                        next_frame = t
                    if display.bar_erased:
                        last_bar = None
                if t >= next_fg:
                    next_fg = t + 0.5
                    fg = os.tcgetpgrp(master)
                    if fg > 0 and fg != pid and not display.alternate:
                        if busy_since is None:
                            busy_since = t
                        elif t - busy_since > 10 and not dozing:
                            act("doze", WIDTH, fit_scale(WIDTH, clamp(display.screen.lines / 33, 0.4, 1)), name, path)
                            dozing = True
                    else:
                        busy_since = None
                        if dozing:
                            act("wake", WIDTH, fit_scale(WIDTH, clamp(display.screen.lines / 33, 0.4, 1)), name, path)
                            dozing = False
                if pending_feed and pending_feed.done():
                    try:
                        message = pending_feed.result()
                    except (ValueError, OSError) as exc:
                        message = str(exc)
                    pending_feed = None
                    until = t + 4
                update = ""
                if t >= next_frame:
                    next_frame = t + 0.08
                    update = cat.frame(display, path, now())
                text = message if t < until else "餵食中…" if pending_feed else status.get() or name
                if text != last_bar:
                    update += cursor(rows - 1, 0) + "\x1b[0;48;5;236;38;5;250m\x1b[2K" + fit_text(" " + text, cols - 1)
                    last_bar = text
                packet = display.packet(erased, shown, update)
                if packet:
                    sys.stdout.buffer.write(packet)
                    sys.stdout.buffer.flush()
        finally:
            if dozing:
                act("wake", WIDTH, 1, name, path)
            emit(display.decorate(cat.paint({}, display)) + "\x1b[r" + cursor(rows - 1, 0) + "\x1b[0m\x1b[2K")


def shell(name, path):
    if "NEKO_SHELL" in os.environ:
        print("已經在 neko shell 裡了。", file=sys.stderr)
        return 1
    cols, rows = os.get_terminal_size(0)
    child, master = pty.fork()
    if child == 0:
        try:
            winsize(0, cols, rows - 1)
            os.environ["NEKO_SHELL"] = "1"
            sh = os.environ.get("SHELL") or "/bin/sh"
            os.execlp(sh, sh)
        except OSError as exc:
            os.write(2, f"neko: {exc}\n".encode())
        os._exit(1)
    try:
        session(child, master, cols, rows, name, path)
    finally:
        os.close(master)
        _, status = os.waitpid(child, 0)
    return os.waitstatus_to_exitcode(status)
