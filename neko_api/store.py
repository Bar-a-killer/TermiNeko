"""讀寫 state.json；和 Rust 版共用同一個檔案。"""

from __future__ import annotations

import fcntl
import json
import os
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from .model import Cat


def default_path() -> Path:
    """和 Rust 版相同：NEKO_STATE，不然就是 ~/neko/state.json。"""
    p = os.environ.get("NEKO_STATE")
    if p:
        return Path(p)
    return Path(os.environ.get("HOME", ".")) / "neko" / "state.json"


class Store:
    def __init__(self, path: Path | str | None = None):
        self.path = Path(path) if path else default_path()

    def load(self, now: float | None = None) -> Cat:
        try:
            d = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            d = {}
        return Cat.from_dict(d, now)

    def save(self, cat: Cat) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(".json.tmp")
        tmp.write_text(json.dumps(cat.to_dict(), ensure_ascii=False, indent=2), encoding="utf-8")
        os.replace(tmp, self.path)  # 原子替換，讀的人不會看到寫一半的檔案
        cat.log = cat.log[-60:]

    @contextmanager
    def edit(self, now: float | None = None) -> Iterator[Cat]:
        """讀出貓、把時間算到現在、交給你改、存回去。同時間只有一個 request 能改。"""
        n = time.time() if now is None else now
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.path.with_suffix(".lock"), "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            cat = self.load(n)
            cat.tick(n)
            # 上次在房間裡碗沒吃完，牠在你不在的時候吃掉了（和 Rust 版開房間時相同）
            cat.finish_bowl(n)
            yield cat
            self.save(cat)
