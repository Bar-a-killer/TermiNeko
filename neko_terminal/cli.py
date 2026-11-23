"""neko                      全螢幕的貓（按 q 離開）
neko shell                貓住在終端機的空白處（exit 離開）
neko line                 印一行狀態
neko portrait             印出 Codex 對話的像素貓
neko pane                 顯示共用動作的像素貓動畫
neko feed [dry|wet|treat] 餵食
neko stream <寬> [倍率] [fps]  Claude Code 動畫串流
neko raster <寬> [倍率]        輸出一格畫面
neko do <動作> <寬> [倍率]     eat pet play call jump nap look paw doze wake
neko pose <姿勢|walk|run> [面向] [倍率] [距離] [扭轉]
neko dump <寬> [倍率]          印出目前畫面
neko trace [寬]               檢查動作銜接及耳尖位置"""

import os
import sys
import time
from math import ceil, isfinite

from neko_api.store import default_path

from . import care
from .animation import act, now, read_pane, state
from .poses import KEYS, clamp, special


def dispatch(args):
    cmd = args[0] if args else ""
    path = default_path().with_name("pane.json")

    def arg(index, default):
        return args[index] if index < len(args) else default

    def number(index, default):
        try:
            n = type(default)(args[index])
            return n if isfinite(n) else default
        except (IndexError, ValueError, OverflowError):
            return default

    if cmd == "line":
        print(care.request("line")["message"])
    elif cmd == "do":
        print(act(arg(1, ""), number(2, 60), number(3, 1.0), care.name(), path))
    elif cmd == "feed":
        print(care.feed(arg(1, "dry"), path))
    elif cmd == "trace":
        from .diagnostics import trace

        trace(number(1, 60))
    elif cmd in ("", "pane"):
        from .terminal import fullscreen

        fullscreen(care.name(), path if cmd else path.with_name("pane-term.json"))
    elif cmd == "shell":
        from .shell import shell

        return shell(care.name(), path)
    elif cmd in ("raster", "stream", "dump", "pose", "portrait"):
        from . import scene

        if cmd == "portrait":
            print(scene.portrait(path), end="")
        elif cmd == "pose":
            label = arg(1, "sit")
            node = label.title()
            if node not in KEYS:
                node = "Stand"
            f, k, d, twist = (number(i, default) for i, default in enumerate((1.0, 1.0, 0.0, 0.0), 2))
            pose = special(KEYS[node], label.title(), 0, d) if label in ("walk", "run") else KEYS[node]
            if k <= 0:
                raise ValueError("倍率必須大於零")
            layer = scene.render(pose, int(40 * k), ceil(22 * k), k, 20.0, f, 0.0, twist)[0]
            print(f"{node} f={f:g}\n" + scene.rows_text(layer), end="")
        elif cmd == "dump":
            w, k = clamp(number(1, 60), 40, 400), number(2, 1.0)
            if k <= 0:
                raise ValueError("倍率必須大於零")
            width = max(40, w / k)
            fr = state(read_pane(path, width), now(), width)
            layer = scene.pixels(fr, w, ceil(22 * k), k)[0]
            print(
                f"{fr.caption}  x={fr.x:.1f} f={fr.f:.2f} node={fr.node} anim={fr.anim}\n" + scene.rows_text(layer),
                end="",
            )
        else:
            w, k, fps = number(1, 60), number(2, 1.0), max(0.01, number(3, 15.0))
            name, status = care.name(), care.StatusLine()
            deadline = time.monotonic() + 12 * 3600
            last, sent = "", 0.0
            while time.monotonic() < deadline:
                start = time.monotonic()
                frame, busy = scene.raster(w, k, name, path, status)
                if cmd == "raster":
                    print(frame, end="")
                    break
                if frame != last or start - sent >= 1:
                    print(frame, end="\x1e", flush=True)
                    last, sent = frame, start
                rate = fps if busy else min(5, fps)
                time.sleep(max(0.005, 1 / rate - (time.monotonic() - start)))
    else:
        print(__doc__)
    return 0


def main(argv=None):
    try:
        return dispatch(sys.argv[1:] if argv is None else argv)
    except BrokenPipeError:
        # Prevent the interpreter from flushing the same broken pipe on exit.
        with open(os.devnull, "wb") as sink:
            os.dup2(sink.fileno(), 1)
        return 0
    except (OSError, ValueError) as exc:
        print(f"neko: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
