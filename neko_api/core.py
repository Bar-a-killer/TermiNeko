"""所有對貓的操作：給 neko-core 指令（終端機版、Claude Code 外掛）用，狀態格式和 HTTP API 共用。

每個操作是一個 dict（`{"op": "feed", "food": "dry"}`），回傳也是 dict：
成功 `{"ok": true, "message": "...", "cat": {...}}`，失敗 `{"ok": false, "error": "..."}`。
這裡不 import FastAPI，啟動才快（終端機版每次開都要叫一次）。
"""

from __future__ import annotations

import time
from typing import Any

from .model import FACTOR, FOODS, IDEAL, Cat, Mood, need

BODY = ["", "過瘦", "過瘦", "偏瘦", "理想", "理想", "過重", "過重", "肥胖", "肥胖"]  # BCS 1–9 的說法
from .store import Store


class OpError(ValueError):
    """操作的參數不對，或貓現在不能做這件事。"""


def status(c: Cat, now: float | None = None) -> dict[str, Any]:
    """貓現在的狀態（畫面需要的都在這裡，終端機版不自己算）。"""
    n = time.time() if now is None else now
    nd = need(c.w)
    return {
        "name": c.name,
        "weight_kg": round(c.w, 3),
        "ideal_kg": IDEAL,
        "bcs": c.bcs(),
        "need_kcal": round(nd, 1),
        "belly_kcal": round(c.belly, 1),
        "belly_pct": round(c.belly / nd * 100, 1),
        "joy": round(c.joy, 1),
        "mood": c.mood().value,
        "face": c.face(),
        "fed_today_kcal": round(c.fed_today, 1),
        "age_days": round((n - c.born) / 86400, 2),
        "bowl_g": round(c.bowl_g, 1),
        # 給人看的說明（API、外掛顯示用）
        "body": BODY[c.bcs()],
        "belly_hours": round(c.belly / nd * 24, 1),
        "need_dry_g": round(nd / FOODS["dry"].kcal_per_kg * 1000),
        "need_formula": f"70 × {c.w:.2f}^0.75 × {FACTOR:g} = {nd:.0f} kcal",
        "last_log": f"{c.log[-1][0]}  {c.log[-1][1]}" if c.log else "",
    }


def line(c: Cat) -> str:
    """一行狀態（tmux 狀態列用），例如 (^.^) 芝麻 3.99kg 肚子17%"""
    m = c.mood()
    face = "(-.-)zZ" if m == Mood.SLEEP else "(>.<)!" if m == Mood.STARVING else f"({c.face()})"
    return f"{face} {c.name} {c.w:.2f}kg 肚子{c.belly / need(c.w) * 100:.0f}%"


def _num(req: dict, key: str, default: float | None = None, lo: float = 0.0, hi: float = 1e9) -> float:
    x = req.get(key, default)
    if x is None:
        raise OpError(f"缺少 {key}")
    try:
        v = float(x)
    except (TypeError, ValueError) as e:
        raise OpError(f"{key} 要是數字") from e
    if not (lo < v <= hi):
        raise OpError(f"{key} 要在 {lo:g}～{hi:g} 之間")
    return v


def _food(req: dict):
    key = req.get("food", "dry")
    if key not in FOODS:
        raise OpError(f"沒有這種食物：{key}（可以用 {'、'.join(FOODS)}）")
    return FOODS[key]


def apply(c: Cat, req: dict, now: float | None = None) -> str | None:
    """對貓做一個操作，回傳要顯示的訊息（沒有就 None）。"""
    op = req.get("op")
    if op == "status":
        return None
    if op == "feed":
        f = _food(req)
        return c.feed(f, _num(req, "grams", f.default_g, hi=500), now)
    if op == "pet":
        return c.pet()
    if op == "play":
        try:
            return c.play(now)
        except ValueError as e:
            raise OpError(str(e)) from e
    if op == "rename":
        try:
            return c.rename(str(req.get("name", "")), now)
        except ValueError as e:
            raise OpError(str(e)) from e
    if op == "advance":
        return c.advance(_num(req, "hours", hi=24 * 30), now)
    if op == "adopt":
        new = Cat.new(req.get("name") or None, now)
        c.__dict__.update(new.__dict__)
        return f"領養了 {c.name}。"
    raise OpError(f"不認得的操作：{op}")


def run(req: dict, store: Store | None = None, now: float | None = None) -> dict[str, Any]:
    """讀檔 → 算到現在 → 做操作 → 存檔，回傳結果。"""
    st = store or Store()
    op = req.get("op")
    try:
        if op == "log":
            limit = int(_num(req, "limit", 20, hi=60))
            with st.edit(now) as c:
                return {"ok": True, "log": [{"time": a, "message": b} for a, b in c.log[-limit:]], "cat": status(c, now)}
        if op == "line":
            with st.edit(now) as c:
                return {"ok": True, "message": line(c), "cat": status(c, now)}
        with st.edit(now) as c:
            msg = apply(c, req, now)
            return {"ok": True, "message": msg, "cat": status(c, now)}
    except OpError as e:
        return {"ok": False, "error": str(e)}
