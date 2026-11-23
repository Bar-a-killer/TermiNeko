"""ANSI output and keyboard handling for the full-screen and tmux views."""

import os
import select
import sys
import termios
import tty
from contextlib import contextmanager
from functools import lru_cache

from wcwidth import wcswidth, wcwidth

from .animation import act, fit_scale, now, read_pane, state, world_width
from .care import StatusLine
from .scene import NONE, scene


@contextmanager
def raw_terminal():
    saved = termios.tcgetattr(0)
    try:
        tty.setraw(0)
        yield
    finally:
        termios.tcsetattr(0, termios.TCSADRAIN, saved)


def emit(text):
    sys.stdout.buffer.write(text.encode())
    sys.stdout.buffer.flush()


@lru_cache(maxsize=1024)
def sgr(fg, bg):
    def component(color, base):
        if color == NONE:
            return f"{base}9"
        return f"{base}8;2;{color >> 16 & 255};{color >> 8 & 255};{color & 255}"

    return f"\x1b[0;{component(fg, 3)};{component(bg, 4)}m"


def fit_text(text, cols):
    used = 0
    for i, char in enumerate(text):
        used += max(0, wcwidth(char))
        if used > cols:
            return text[:i]
    return text


def cursor(row, col):
    return f"\x1b[{row + 1};{col + 1}H"


def fullscreen(name, path):
    status = StatusLine()
    zoom = 1.0
    message, until = "", 0.0
    last_size = None
    actions = {
        c: what
        for keys, what in [("e1", "eat"), ("p2", "pet"), ("b3", "play"), ("c4", "call"), ("j5 ", "jump"), ("n6", "nap")]
        for c in keys
    }
    with raw_terminal():
        try:
            emit("\x1b[?1049h\x1b[?25l\x1b[2J")
            while True:
                w, rows = os.get_terminal_size(0)
                out = []
                if (w, rows) != last_size:
                    out.append("\x1b[0m\x1b[2J\x1b[H")
                    last_size = w, rows
                k = fit_scale(w, (rows - 1) * 2 / 30 * zoom)
                if w < 40 or rows < 8:
                    out.append("\x1b[H視窗太小了（至少 40×8），q 離開")
                else:
                    width = world_width(w, k)
                    t = now()
                    frame = state(read_pane(path, width), t, width)
                    last = None
                    for r, row in enumerate(scene(frame, w, 2 * (rows - 1), k, t)):
                        out.append(cursor(r, 0))
                        for ch, fg, bg in row.tolist():
                            if (fg, bg) != last:
                                out.append(sgr(fg, bg))
                                last = fg, bg
                            out.append(chr(ch))
                    nutrition = status.get().replace(f" {name} ", " ", 1)
                    left = "  " + (message if t < until else f"{name} {frame.caption}  {nutrition}")
                    keys = f"e 吃飯  p 摸摸  b 丟球  c 叫牠  j 跳  n 哄睡  +/- 大小（×{k:.1f}）  q 離開 "
                    if wcswidth(left + keys) + 2 > w:
                        keys = "e吃 p摸 b球 c叫 j跳 n睡 ±大小 q離開 "
                    left = fit_text(left, max(0, w - wcswidth(keys) - 1))
                    out.extend(
                        (
                            cursor(rows - 1, 0),
                            "\x1b[0;2m",
                            left,
                            " " * max(0, w - wcswidth(left + keys)),
                            keys,
                            "\x1b[0m",
                        )
                    )
                emit("".join(out))
                if select.select([0], [], [], 0.033)[0]:
                    data = os.read(0, 1024)
                    if not data or data == b"\x1b" or b"\x03" in data:
                        return
                    if data.startswith(b"\x1b"):
                        continue  # Navigation keys carry an ESC prefix.
                    for ch in data.decode(errors="ignore"):
                        if ch == "q":
                            return
                        if ch in actions and w >= 40 and rows >= 8:
                            message = act(actions[ch], w, k, name, path)
                            until = now() + 3
                        elif ch in "+=":
                            zoom = min(4, zoom * 1.25)
                        elif ch in "-_":
                            zoom = max(0.2, zoom / 1.25)
                        elif ch == "0":
                            zoom = 1.0
        finally:
            emit("\x1b[0m\x1b[?25h\x1b[?1049l")
