"""Compose props and captions and encode the Claude Code Raster cells."""

import base64
from math import ceil, floor

import numpy as np

from .animation import fit_scale, now, read_pane, state, world_width
from .poses import clamp
from .render import render

NONE = 0x01000000
PALETTE = {
    "k": 0x3A2618,
    "o": 0xF2A250,
    "s": 0xD8873A,
    "t": 0xB36A2A,
    "g": 0xE8974A,
    "V": 0xB9A690,
    "P": 0xD98494,
    "l": 0xFFC47E,
    "d": 0xCC7A2C,
    "D": 0xC27026,
    "K": 0x8F5220,
    "w": 0xFFF6EA,
    "W": 0xE0D2BF,
    "p": 0xF4A0B0,
    "n": 0xE7798C,
    "e": 0x2B1D14,
    "a": 0xA9C552,
    "m": 0x9A5440,
    "M": 0x5A2324,
    "b": 0x5B8DD9,
    "B": 0x3F6FB8,
    "f": 0x9A6232,
    "r": 0xE0483C,
    "R": 0xFF9A8A,
}
COLORS = np.full(256, NONE, dtype=np.uint32)
for char, color in PALETTE.items():
    COLORS[ord(char)] = color


def pixels(frame, w, h, k):
    return render(frame.pose, w, h, k, frame.x, frame.f, frame.lift, frame.twist)


def art(layer, k, x, top, rows):
    h, w = layer.shape
    for j, row in enumerate(rows):
        bottom = top - j
        y0, y1 = h - floor((bottom + 1) * k), h - floor(bottom * k)
        for i, char in enumerate(row):
            if char != ".":
                x0, x1 = floor((x + i) * k), floor((x + i + 1) * k)
                layer[max(0, y0) : max(0, min(h, y1)), max(0, x0) : max(0, min(w, x1))] = ord(char)


def scene(frame, w, h, k, t):
    cat, hx, hy = pixels(frame, w, h, k)
    if frame.bowl:
        bx, level = frame.bowl
        x = floor(bx + 0.5) - 4
        layer = np.full_like(cat, 32)
        if level > 0.66:
            art(layer, k, x + 2, 4, ["ffff"])
        if level > 0.02:
            art(layer, k, x + 1, 3, ["ffffff" if level > 0.33 else ".ff.f."])
        layer = np.where(cat != 32, cat, layer)
        art(layer, k, x, 3, ["B......B", "BbbbbbbB", ".BbbbbB.", "..kkkk.."])
    else:
        layer = cat
    if frame.ball:
        bx, height = frame.ball
        art(layer, k, floor(bx + 0.5) - 1, 3 + floor(height + 0.5), [".rr.", "rRrr", ".rr."])
    top, bottom = COLORS[layer[::2]], COLORS[layer[1::2]]
    cells = np.empty((h // 2, w, 3), dtype="<u4")
    cells[:, :, 0] = np.where(top != NONE, 0x2580, np.where(bottom != NONE, 0x2584, 32))
    cells[:, :, 1] = np.where(top == NONE, bottom, top)
    cells[:, :, 2] = np.where(top != NONE, bottom, NONE)
    hc, hr = floor(hx + 0.5), floor(hy / 2)
    direction = -1 if frame.f < 0 else 1

    def ki(n):
        return floor(n * k + 0.5)

    def label(x, y, string, rgb):
        if 0 <= y < h // 2:
            for i, char in enumerate(string, x):
                if 0 <= i < w and cells[y, i, 0] == 32:
                    cells[y, i] = ord(char), rgb, NONE

    if frame.node == "Sleep":
        label(hc + direction * ki(4), max(0, hr - ki(2)), ("z", "zZ", "zZz", "")[int(t * 1.3) % 4], 0x9AA7B8)
    if frame.anim == "Happy":
        for n in range(3):
            phase = int((t * 1.6 + n * 0.9) % 3)
            label(hc + direction * ki(4 + n * 3) + phase % 2, max(0, hr - phase * ki(1)), "♥", 0xF4728B)
    if frame.anim == "Meow" and int(t * 3) % 3 != 2:
        label(hc + ki(5) if direction > 0 else hc - ki(5) - 5, max(0, hr - ki(1)), "meow!", 0xE6E6E6)
    if frame.anim == "Eat" and int(t * 3) % 3 and frame.bowl:
        label(ki(frame.bowl[0]) + ki(3), int((h - 1 - 9 * k) / 2), "nom", 0xC9A26B)
    return cells


def raster(cols, k, name, path, status):
    w = clamp(cols, 40, 400)
    k = fit_scale(w, k)
    rows = ceil(11 * k)
    width = world_width(w, k)
    t = now()
    fr = state(read_pane(path, width), t, width)
    data = base64.b64encode(scene(fr, w, rows * 2, k, t).tobytes()).decode("ascii")
    nutrition = status.get().replace(f" {name} ", " ", 1)
    busy = fr.anim not in ("Idle", "Still", "Sleep") or fr.lift != 0 or fr.twist != 0
    return f"{w} {rows}\n{name} {fr.caption}\t{nutrition}\n{data}\n", busy


def rows_text(layer):
    return "".join(row.tobytes().decode("ascii").rstrip() + "\n" for row in layer)


def portrait(path):
    w, k = 48, 0.8
    width = world_width(w, k)
    fr = state(read_pane(path, width), now(), width)
    layer = pixels(fr, w, ceil(22 * k), k)[0]
    left = np.nonzero(layer != 32)[1].min(initial=w)
    colors = {
        c: emoji
        for chars, emoji in [
            (" ", "　"),
            ("kKdDtm", "🟫"),
            ("eM", "⬛"),
            ("wWV", "⬜"),
            ("npP", "🟥"),
            ("a", "🟩"),
            ("l", "🟨"),
        ]
        for c in chars
    }
    return "".join(
        "".join(colors.get(c, "🟧") for c in row[int(left) :]) + "\n" for row in rows_text(layer).splitlines() if row
    )
