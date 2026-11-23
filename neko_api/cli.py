"""neko-core：終端機版和 Claude Code 外掛用來問貓、對貓做事的指令。

    neko-core status                 貓現在的狀態（JSON）
    neko-core line                   一行狀態（純文字，給 tmux 狀態列）
    neko-core feed dry [克]          餵食，馬上吃完（dry / wet / treat）
    neko-core pet | play             摸摸、丟球
    neko-core rename 小黑            改名
    neko-core advance 6              快轉 6 小時（測試用）
    neko-core adopt [名字]           重新領養
    neko-core log [筆數]             最近的紀錄

計算和存檔全部在 neko_api.core / neko_api.model，這裡只負責解析指令。
"""

from __future__ import annotations

import json
import sys

from . import core
from .store import Store


def _parse(argv: list[str]) -> dict:
    if not argv:
        return {"op": "status"}
    op, rest = argv[0], argv[1:]
    req: dict = {"op": op}
    if op == "feed":
        if rest:
            req["food"] = rest[0]
        if len(rest) > 1:
            req["grams"] = rest[1]
    elif op == "rename":
        req["name"] = " ".join(rest)
    elif op == "advance":
        req["hours"] = rest[0] if rest else None
    elif op == "adopt":
        req["name"] = " ".join(rest) or None
    elif op == "log":
        if rest:
            req["limit"] = rest[0]
    return req


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if args[:1] in (["-h"], ["--help"], ["help"]):
        print(__doc__)
        return 0
    res = core.run(_parse(args), Store())
    if args[:1] == ["line"] and res.get("ok"):
        print(res["message"])
    else:
        print(json.dumps(res, ensure_ascii=False))
    return 0 if res.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main())
