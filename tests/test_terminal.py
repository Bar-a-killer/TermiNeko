"""Compatibility with recorded Rust output, existing pane files and the Claude Code wire format."""

import base64
import hashlib
import json

import pytest

from neko_terminal.animation import act, read_pane, state
from neko_terminal.cli import main
from neko_terminal.shell import SYNC_OFF, SYNC_ON, Display, Overlay


@pytest.mark.parametrize(
    ("args", "digest"),
    [
        (["walk", "-1", "1", "2.3"], "02f64a1f71e5579ade1fca609a17a30541c0f57e87c651a4d1a12faf6b7fa56c"),
        (["happy", "0", "2"], "a82ee90ee9d0ee9eba26623de0bff12e4a89c058d6c7be2114d015c40e426c75"),
        (["sit", "0.4", "1.5", "0", "0.6"], "4a16151d80d73e21f2ed9791f7fa4c8c4de312aa911f64d61f793687b647b9f9"),
    ],
)
def test_reference_poses(args, digest, capsys):
    assert main(["pose", *args]) == 0
    assert hashlib.sha256(capsys.readouterr().out.encode()).hexdigest() == digest


def test_claude_raster(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("NEKO_STATE", str(tmp_path / "state.json"))
    monkeypatch.delenv("NEKO_CORE", raising=False)
    monkeypatch.setenv("NEKO_NOW", "1763370004")
    assert main(["raster", "60", "1"]) == 0
    size, caption, encoded = capsys.readouterr().out.splitlines()
    cells = base64.b64decode(encoded, validate=True)
    assert size == "60 11"
    assert "\t" in caption
    assert len(cells) == 60 * 11 * 12
    assert hashlib.sha256(cells).hexdigest() == "c0ed20e4acef9da7ef089b2af88bb39c84368d59b669b9e36ef0c57d8f2be005"


def test_interrupt_legacy_sleep_after_resize(tmp_path, monkeypatch):
    path = tmp_path / "pane.json"
    path.write_text(json.dumps({"act": "doze", "t": 100.0, "w": 100}))
    monkeypatch.setenv("NEKO_NOW", "130")
    before = state(read_pane(path, 40), 130, 40)
    assert before.node == "Sleep"
    act("wake", 40, 1, "小黑", path)
    saved = json.loads(path.read_text())
    assert saved["from"]["p"] == before.core.array()
    assert saved["from"]["x"] == before.x
    after = state(read_pane(path, 40), 130, 40)
    assert after.pose == before.pose


def test_shell_buffers_split_sequences_and_restores_primary_screen():
    display = Display(80, 23)
    assert display.process(b"hello\x1b[") == b"hello"
    assert display.process(b"r") == b"\x1b[1;23r"
    display.process(b"\x1b[?1049h")
    assert display.alternate
    display.process("編輯".encode())
    display.process(b"\x1b[?1049l")
    assert not display.alternate
    assert display.screen.display[0].startswith("hello")
    assert not display.blank(0, 0, 5, 1)
    assert display.blank(5, 0, 40, 1)


def test_prompt_edits_leave_cat_and_bar_intact():
    display = Display(80, 23)
    cat = Overlay()
    cat.drawn = {(10, 50): (ord("▀"), 0x123456, 0)}
    # ZLE sends these in separate PTY reads, including split UTF-8 and CSI.
    parts = [
        b"neko% ",
        b"\xe8",
        b"\xb2\x93",
        b"\x08",
        b"\x1b[",
        b"K",
        b"\x1b[31m",
        b"a\x1b[0m",
        b"\x1b[11;60H\x1b[K",
        b"\x1b[1;1H\n",
    ]
    sent = bytearray()
    for part in parts:
        erase, shown = cat.forward(display, part)
        assert erase == b""
        assert cat.drawn
        assert not display.bar_erased
        sent.extend(shown)
    assert sent == b"".join(parts)


@pytest.mark.parametrize(
    "output",
    [
        b"\x1b[11;51H ",  # A space overwrites a pixel on a virtually blank cell.
        b"\x1b[11;51H\x1b[K",  # Erase through the cat.
        b"\x1b[23;1H\n",  # Scroll a screen with entirely blank underlying rows.
        b"\x1b[S",  # Explicit scroll.
        b"\x1b[?1049h",  # Restore the primary surface before entering the alternate one.
    ],
)
def test_output_displacing_the_cat_restores_old_surface(output):
    display = Display(80, 23)
    cat = Overlay()
    cat.drawn = {(10, 50): (ord("▀"), 0x123456, 0)}
    erase, shown = cat.forward(display, output)
    assert b"\x1b[11;51H" in erase
    assert shown == output
    assert not cat.drawn


def test_cursor_restoration_preserves_wide_glyph_at_wrap_boundary():
    display = Display(8, 4)
    display.process("123456貓".encode())
    assert display.screen.cursor.x == 8
    display.process(display.decorate("\x1b[3;2H!").encode())
    display.process(b"a")
    assert display.screen.display[:2] == ["123456貓", "a       "]


def test_shell_and_cat_finish_one_synchronized_update():
    display = Display(80, 23)
    cat = Overlay()
    cat.drawn = {(10, 50): (ord("▀"), 0x123456, 0)}
    assert display.packet(*cat.forward(display, SYNC_ON), "") == SYNC_ON
    data = b"\x1b[2J"
    packet = display.packet(*cat.forward(display, data + SYNC_OFF), "▀")
    assert display.bar_erased
    assert packet.index(b"\x1b[11;51H") < packet.index(data) < packet.index("▀".encode())
    assert packet.count(SYNC_OFF) == 1 and packet.endswith(SYNC_OFF)
    assert display.packet(*cat.forward(display, b"a"), "") == b"a"
