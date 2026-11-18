"""貓的模型：營養、心情、紀錄的所有計算都在這裡（Rust 只負責畫面，透過 neko-core 呼叫這裡）。

每日需求 = 70 × 體重^0.75 × 1.2（結紮成貓），體重每變化 1 kg ≈ 7000 kcal。
所有跟時間有關的函式都吃一個 `now`（Unix 秒），方便測試和模擬。
"""

from __future__ import annotations

import random
import time
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum

KCAL_PER_KG = 7000.0  # 體重每變化 1 kg 約等於 7000 kcal
FACTOR = 1.2  # 結紮成貓
IDEAL = 4.0  # 理想體重（kg）
FLOOR = IDEAL * 0.72  # 體重下限：太久沒回來，鄰居會幫忙餵
LOG_LIMIT = 60  # 存檔時只保留最近幾筆紀錄
NAMES = ["麻糬", "芝麻", "布丁", "饅頭", "年糕", "豆花", "肉鬆"]


def need(w: float) -> float:
    """體重 w 公斤的貓一天需要多少 kcal。"""
    return 70.0 * w**0.75 * FACTOR


def stamp(now: float) -> str:
    return datetime.fromtimestamp(now).strftime("%m-%d %H:%M")


def date_of(now: float) -> str:
    return datetime.fromtimestamp(now).strftime("%Y-%m-%d")


@dataclass(frozen=True)
class Food:
    key: str
    label: str
    kcal_per_kg: float
    default_g: float  # 沒指定份量時給多少克
    joy: float = 0.0  # 給的時候額外加多少心情


FOODS: dict[str, Food] = {
    "dry": Food("dry", "乾糧", 3800.0, 10.0),
    "wet": Food("wet", "濕食", 1000.0, 40.0),
    "treat": Food("treat", "零食", 3750.0, 4.0, joy=10.0),
}


class Mood(str, Enum):
    STARVING = "starving"
    HUNGRY = "hungry"
    FULL = "full"
    SLEEP = "sleep"
    BORED = "bored"
    HAPPY = "happy"
    OK = "ok"


FACES = {
    Mood.STARVING: ">.<",
    Mood.HUNGRY: ";.;",
    Mood.FULL: "-.-",
    Mood.SLEEP: "-.-",
    Mood.BORED: "-_-",
    Mood.HAPPY: "^.^",
    Mood.OK: "o.o",
}


@dataclass
class Cat:
    name: str
    w: float  # 體重（kg）
    belly: float  # 肚子裡還有多少 kcal
    joy: float  # 心情 0–100
    born: float  # 領養時間（Unix 秒）
    t: float  # 上次計算到的時間
    fed_today: float  # 今天吃了多少 kcal
    day: str  # fed_today 是哪一天的
    log: list[tuple[str, str]] = field(default_factory=list)
    bowl_g: float = 0.0
    bowl_kcal: float = 0.0
    # 正在吃的這一餐（分幾口吃時，吃完碗才結算成一筆紀錄）
    meal_g: float = 0.0
    meal_kcal: float = 0.0
    meal_extra: float = 0.0

    # ───────── 建立與存讀 ─────────

    @classmethod
    def new(cls, name: str | None = None, now: float | None = None) -> Cat:
        n = time.time() if now is None else now
        c = cls(
            name=name or NAMES[int(n) % len(NAMES)],
            w=IDEAL,
            belly=need(IDEAL) / 3.0,
            joy=70.0,
            born=n,
            t=n,
            fed_today=0.0,
            day=date_of(n),
        )
        c.note(f"領養了 {c.name}", n)
        return c

    @classmethod
    def from_dict(cls, d: dict, now: float | None = None) -> Cat:
        """讀 state.json 的內容；格式壞掉或沒有體重就重新領養。"""
        n = time.time() if now is None else now
        w = d.get("w") if isinstance(d, dict) else None
        if not isinstance(w, (int, float)) or w <= 0:
            return cls.new(now=n)

        def f(k: str, default: float) -> float:
            x = d.get(k)
            if isinstance(x, (int, float)) and x == x and abs(x) != float("inf"):
                return float(x)
            return default

        def s(k: str, default: str) -> str:
            x = d.get(k)
            return x if isinstance(x, str) else default

        log = []
        for e in d.get("log") or []:
            if isinstance(e, list) and len(e) >= 2 and isinstance(e[0], str) and isinstance(e[1], str):
                log.append((e[0], e[1]))
        return cls(
            name=s("name", "貓"),
            w=f("w", IDEAL),
            belly=max(f("belly", 0.0), 0.0),
            joy=min(max(f("joy", 50.0), 0.0), 100.0),
            born=f("born", n),
            t=f("t", n),
            fed_today=f("fed_today", 0.0),
            day=s("day", date_of(n)),
            log=log,
            bowl_g=max(f("bowl_g", 0.0), 0.0),
            bowl_kcal=max(f("bowl_kcal", 0.0), 0.0),
            meal_g=max(f("meal_g", 0.0), 0.0),
            meal_kcal=max(f("meal_kcal", 0.0), 0.0),
            meal_extra=max(f("meal_extra", 0.0), 0.0),
        )

    def to_dict(self) -> dict:
        return {
            "name": self.name,
            "w": self.w,
            "belly": self.belly,
            "joy": self.joy,
            "born": self.born,
            "t": self.t,
            "fed_today": self.fed_today,
            "day": self.day,
            "bowl_g": self.bowl_g,
            "bowl_kcal": self.bowl_kcal,
            "meal_g": self.meal_g,
            "meal_kcal": self.meal_kcal,
            "meal_extra": self.meal_extra,
            "log": [list(e) for e in self.log[-LOG_LIMIT:]],
        }

    # ───────── 規則 ─────────

    def note(self, msg: str, now: float | None = None) -> None:
        self.log.append((stamp(time.time() if now is None else now), msg))

    def pass_time(self, dt: float, now: float | None = None) -> None:
        """經過 dt 秒：消耗熱量、肚子空了就掉體重、沒人陪就無聊。"""
        n = time.time() if now is None else now
        d = date_of(n)
        if d != self.day:
            self.day = d
            self.fed_today = 0.0
        days = dt / 86400.0
        self.belly -= need(self.w) * days
        if self.belly < 0.0:
            self.w += self.belly / KCAL_PER_KG
            self.belly = 0.0
        if self.w < FLOOR:
            self.w = FLOOR
            if days > 2.0:
                self.note("你太久沒回來，鄰居幫忙餵了幾天", n)
        self.joy = max(self.joy - 1.5 * dt / 3600.0, 0.0)

    def tick(self, now: float | None = None) -> None:
        """把真實時間算進去（上次計算到現在）。"""
        n = time.time() if now is None else now
        dt = max(n - self.t, 0.0)
        self.t = n
        self.pass_time(dt, n)

    def eat(self, kcal: float) -> float:
        """吃下 kcal；一天份以上的熱量存不住，直接變成體重。回傳多出來的 kcal。"""
        self.belly += kcal
        self.fed_today += kcal
        extra = self.belly - need(self.w)
        if extra > 0.0:
            self.w += extra / KCAL_PER_KG
            self.belly = min(need(self.w), self.belly - extra)
            return extra
        return 0.0

    def put_in_bowl(self, food: Food, grams: float) -> str:
        """放食物進碗裡（還沒吃）。零食會讓牠馬上開心一點。"""
        kcal = grams * food.kcal_per_kg / 1000.0
        self.joy = min(self.joy + food.joy, 100.0)
        self.bowl_g += grams
        self.bowl_kcal += kcal
        return f"碗裡加了 {grams:.0f} 克{food.label}（{kcal:.0f} kcal）"

    def eat_from_bowl(self, grams: float, now: float | None = None) -> str | None:
        """從碗裡吃 grams 克（可以分幾口吃）。

        吃到碗空的那一口結算這一餐、寫一筆紀錄，回傳訊息；還沒吃完或碗是空的回傳 None。
        """
        if self.bowl_g <= 0.0 and self.bowl_kcal <= 0.0:
            return None
        g = min(max(grams, 0.0), self.bowl_g)
        k = self.bowl_kcal * g / self.bowl_g if self.bowl_g > 0.0 else self.bowl_kcal
        self.bowl_g -= g
        self.bowl_kcal -= k
        if self.bowl_g < 0.05:
            # 碗底剩的一點點一起吃掉
            g += self.bowl_g
            k += self.bowl_kcal
            self.bowl_g = self.bowl_kcal = 0.0
        self.meal_extra += self.eat(k)
        self.meal_g += g
        self.meal_kcal += k
        if self.bowl_g > 0.0:
            return None
        g, k, extra = self.meal_g, self.meal_kcal, self.meal_extra
        self.meal_g = self.meal_kcal = self.meal_extra = 0.0
        msg = f"吃了 {g:.0f} 克（{k:.0f} kcal）"
        if extra > 0.0:
            msg += f"，吃太撐，多的 {extra:.0f} kcal 變成了 {extra / KCAL_PER_KG * 1000:.0f} 克體重"
            self.joy = max(self.joy - 5.0, 0.0)
        else:
            self.joy = min(self.joy + 6.0, 100.0)
        self.note(msg, now)
        return f"{self.name} {msg}"

    def finish_bowl(self, now: float | None = None) -> str | None:
        """把碗裡的東西一次吃完，寫一筆紀錄，回傳訊息；碗是空的就回傳 None。"""
        return self.eat_from_bowl(self.bowl_g, now)

    def feed(self, food: Food, grams: float, now: float | None = None) -> str:
        """放食物進碗裡，貓馬上吃完。"""
        self.put_in_bowl(food, grams)
        return self.finish_bowl(now) or ""

    def pet(self) -> str:
        self.joy = min(self.joy + 3.0, 100.0)
        return f"你摸了摸 {self.name}。"

    def play(self, now: float | None = None) -> str:
        """丟球。餓壞了不玩，會丟出 ValueError。"""
        if self.belly <= 0.0:
            raise ValueError(f"{self.name} 餓到不想玩。先餵牠。")
        self.joy = min(self.joy + 18.0, 100.0)
        self.belly = max(self.belly - 6.0, 0.0)
        self.note("玩了球", now)
        return f"{self.name} 抓到球了！（消耗 6 kcal）"

    def advance(self, hours: float, now: float | None = None) -> str:
        """快轉時間（測試用）：年齡也跟著加。"""
        dt = hours * 3600.0
        self.pass_time(dt, now)
        self.born -= dt
        return f"快轉 {hours:g} 小時。"

    def rename(self, name: str, now: float | None = None) -> str:
        n = name.strip()[:12]
        if not n:
            raise ValueError("名字不能是空的")
        msg = f"{self.name} 改名叫 {n}"
        self.note(msg, now)
        self.name = n
        return msg

    # ───────── 狀態 ─────────

    def bcs(self) -> int:
        """體態分數 1–9，5 是理想；每偏離理想體重 10% 差一級。"""
        return min(max(round(5.0 + (self.w / IDEAL - 1.0) / 0.1), 1), 9)

    def mood(self, hour: int | None = None) -> Mood:
        h = datetime.now().hour if hour is None else hour
        n = need(self.w)
        if self.belly <= 0.0:
            return Mood.STARVING
        if self.belly < n * 0.08:
            return Mood.HUNGRY
        if self.belly > n * 0.9:
            return Mood.FULL
        if h >= 23 or h < 7 or 13 <= h < 15:
            return Mood.SLEEP
        if self.joy < 25.0:
            return Mood.BORED
        if self.joy > 60.0:
            return Mood.HAPPY
        return Mood.OK

    def face(self, hour: int | None = None) -> str:
        return FACES[self.mood(hour)]


# ───────── 模擬（不碰存檔） ─────────


def simulate(
    start_w: float,
    daily_kcal: float,
    days: int,
    meals_per_day: int = 3,
    jitter: float = 0.0,
    seed: int | None = None,
) -> list[dict]:
    """用同一套規則跑 days 天，回傳每天結束時的體重。

    jitter：每餐份量隨機上下浮動的比例（0.1 = ±10%），模擬挑食或偷吃。
    """
    rng = random.Random(seed)
    c = Cat(name="sim", w=start_w, belly=need(start_w) / 3.0, joy=70.0, born=0.0, t=0.0, fed_today=0.0, day="sim")
    gap = 86400.0 / meals_per_day
    out = [{"day": 0, "w": round(c.w, 4), "bcs": c.bcs(), "need": round(need(c.w), 1)}]
    for d in range(1, days + 1):
        for _ in range(meals_per_day):
            c.belly -= need(c.w) * gap / 86400.0
            if c.belly < 0.0:
                c.w += c.belly / KCAL_PER_KG
                c.belly = 0.0
            c.w = max(c.w, FLOOR)
            portion = daily_kcal / meals_per_day
            if jitter:
                portion *= 1.0 + rng.uniform(-jitter, jitter)
            c.eat(portion)
        out.append({"day": d, "w": round(c.w, 4), "bcs": c.bcs(), "need": round(need(c.w), 1)})
    return out
