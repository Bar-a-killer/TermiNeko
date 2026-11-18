"""neko 的 HTTP API（FastAPI）。

    uvicorn neko_api.server:app --reload
    # 或
    neko-api --port 8000

文件：啟動後打開 http://127.0.0.1:8000/docs
"""

from __future__ import annotations

from typing import Literal

from fastapi import FastAPI, HTTPException, Query
from pydantic import BaseModel, Field

from . import __version__, core
from .model import FOODS, IDEAL, Cat, need, simulate
from .store import Store

# ───────── 回傳格式 ─────────


class CatStatus(BaseModel):
    name: str
    weight_kg: float = Field(description="體重")
    ideal_kg: float = Field(description="理想體重")
    bcs: int = Field(description="體態分數 1–9，5 是理想")
    need_kcal: float = Field(description="目前體重一天需要多少 kcal")
    belly_kcal: float = Field(description="肚子裡還有多少 kcal")
    belly_pct: float = Field(description="肚子飽到幾 %（相對一天份）")
    joy: float = Field(description="心情 0–100")
    mood: str = Field(description="starving / hungry / full / bored / happy / ok")
    face: str
    fed_today_kcal: float
    age_days: float = Field(description="領養幾天了")
    bowl_g: float
    body: str = Field(description="體態：過瘦 / 偏瘦 / 理想 / 過重 / 肥胖")
    belly_hours: float = Field(description="肚子裡的熱量還能撐幾小時")
    need_dry_g: float = Field(description="一天的需求換算成乾糧幾克")
    need_formula: str = Field(description="每日需求的算式")
    last_log: str = Field(description="最新一筆紀錄")


class ActionResult(BaseModel):
    message: str
    cat: CatStatus


class LogEntry(BaseModel):
    time: str
    message: str


class FoodInfo(BaseModel):
    key: str
    label: str
    kcal_per_kg: float
    default_g: float


class FeedRequest(BaseModel):
    food: Literal["dry", "wet", "treat"] = "dry"
    grams: float | None = Field(default=None, gt=0, le=500, description="不填就用預設份量")


class RenameRequest(BaseModel):
    name: str = Field(min_length=1, max_length=12)


class AdvanceRequest(BaseModel):
    hours: float = Field(gt=0, le=24 * 30, description="快轉幾小時")


class AdoptRequest(BaseModel):
    name: str | None = Field(default=None, max_length=12)
    confirm: bool = Field(default=False, description="必須是 true，目前這隻貓會被取代")


class SimRequest(BaseModel):
    start_weight: float = Field(default=IDEAL, gt=0.5, le=15)
    daily_kcal: float = Field(gt=0, le=2000, description="每天餵多少 kcal")
    days: int = Field(default=90, ge=1, le=3650)
    meals_per_day: int = Field(default=3, ge=1, le=24)
    jitter: float = Field(default=0.0, ge=0, le=1, description="每餐份量隨機浮動比例")
    seed: int | None = None


class SimPoint(BaseModel):
    day: int
    w: float
    bcs: int
    need: float


class Nutrition(BaseModel):
    weight_kg: float
    need_kcal: float
    bcs: int
    dry_g: float = Field(description="只吃乾糧的話一天幾克")
    wet_g: float = Field(description="只吃濕食的話一天幾克")


# ───────── App ─────────


def status(c: Cat, now: float | None = None) -> CatStatus:
    return CatStatus(**core.status(c, now))


def create_app(store: Store | None = None) -> FastAPI:
    st = store or Store()
    app = FastAPI(
        title="neko API",
        version=__version__,
        description="住在終端機裡的貓。和終端機版共用 state.json，時間是真的在走。",
    )

    @app.get("/cat", response_model=CatStatus, tags=["cat"], summary="貓現在的狀態")
    def get_cat():
        with st.edit() as c:
            return status(c)

    @app.get("/cat/log", response_model=list[LogEntry], tags=["cat"], summary="最近的紀錄（新的在後）")
    def get_log(limit: int = Query(20, ge=1, le=60)):
        with st.edit() as c:
            return [LogEntry(time=a, message=b) for a, b in c.log[-limit:]]

    @app.post("/cat/feed", response_model=ActionResult, tags=["actions"], summary="餵食")
    def feed(req: FeedRequest):
        food = FOODS[req.food]
        with st.edit() as c:
            msg = c.feed(food, req.grams or food.default_g)
            return ActionResult(message=msg, cat=status(c))

    @app.post("/cat/pet", response_model=ActionResult, tags=["actions"], summary="摸摸（心情 +3）")
    def pet():
        with st.edit() as c:
            msg = c.pet()
            return ActionResult(message=msg, cat=status(c))

    @app.post(
        "/cat/play",
        response_model=ActionResult,
        tags=["actions"],
        summary="丟球（心情 +18，消耗 6 kcal）",
        responses={409: {"description": "貓餓壞了不想玩"}},
    )
    def play():
        with st.edit() as c:
            try:
                msg = c.play()
            except ValueError as e:
                raise HTTPException(409, str(e)) from e
            return ActionResult(message=msg, cat=status(c))

    @app.post("/cat/rename", response_model=ActionResult, tags=["actions"], summary="改名")
    def rename(req: RenameRequest):
        with st.edit() as c:
            try:
                msg = c.rename(req.name)
            except ValueError as e:
                raise HTTPException(422, str(e)) from e
            return ActionResult(message=msg, cat=status(c))

    @app.post("/cat/advance", response_model=ActionResult, tags=["debug"], summary="快轉時間（測試用）")
    def advance(req: AdvanceRequest):
        with st.edit() as c:
            msg = c.advance(req.hours)
            return ActionResult(message=msg, cat=status(c))

    @app.post(
        "/cat/adopt",
        response_model=ActionResult,
        tags=["debug"],
        summary="重新領養（目前這隻會離開）",
        responses={400: {"description": "沒有 confirm"}},
    )
    def adopt(req: AdoptRequest):
        if not req.confirm:
            raise HTTPException(400, "要帶 confirm: true，目前這隻貓會離開")
        with st.edit() as c:
            new = Cat.new(req.name)
            c.__dict__.update(new.__dict__)
            return ActionResult(message=f"領養了 {c.name}。", cat=status(c))

    @app.get("/foods", response_model=list[FoodInfo], tags=["info"], summary="食物種類")
    def foods():
        return [
            FoodInfo(key=f.key, label=f.label, kcal_per_kg=f.kcal_per_kg, default_g=f.default_g) for f in FOODS.values()
        ]

    @app.get("/nutrition", response_model=Nutrition, tags=["info"], summary="某個體重一天該吃多少")
    def nutrition(weight: float = Query(..., gt=0.5, le=15, description="體重（kg）")):
        nd = need(weight)
        bcs = min(max(round(5.0 + (weight / IDEAL - 1.0) / 0.1), 1), 9)
        return Nutrition(
            weight_kg=weight,
            need_kcal=round(nd, 1),
            bcs=bcs,
            dry_g=round(nd / FOODS["dry"].kcal_per_kg * 1000, 1),
            wet_g=round(nd / FOODS["wet"].kcal_per_kg * 1000, 1),
        )

    @app.post(
        "/simulate", response_model=list[SimPoint], tags=["info"], summary="模擬固定餵法下的體重變化（不影響存檔）"
    )
    def sim(req: SimRequest):
        return simulate(req.start_weight, req.daily_kcal, req.days, req.meals_per_day, req.jitter, req.seed)

    return app


app = create_app()


def main() -> None:
    import argparse

    import uvicorn

    p = argparse.ArgumentParser(description="neko HTTP API")
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8000)
    p.add_argument("--reload", action="store_true")
    a = p.parse_args()
    uvicorn.run("neko_api.server:app", host=a.host, port=a.port, reload=a.reload)


if __name__ == "__main__":
    main()
