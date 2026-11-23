"""Animation timelines, independent of the terminal surface and render rate."""

import json
import os
import struct
import tempfile
import time
from collections import deque
from dataclasses import dataclass, replace
from functools import lru_cache
from math import copysign, cos, floor, isfinite, pi, sin
from pathlib import Path

from .poses import KEYS, NODES, Pose, clamp, common, mix, smooth, special


def now():
    try:
        t = float(os.environ.get("NEKO_NOW", ""))
        if isfinite(t):
            return t
    except ValueError:
        pass
    return time.time()


def seed_at(t, salt=0):
    mask = (1 << 64) - 1
    n = struct.unpack("<Q", struct.pack("<d", t))[0] ^ salt
    x = ((n * 0x9E3779B97F4A7C15) & mask) ^ 0xD1B54A32D192ED03
    x = ((x ^ (x >> 29)) * 0xBF58476D1CE4E5B9) & mask
    return x ^ (x >> 32)


def fit_scale(width, scale):
    low = max(width / 160, 0.4)
    return clamp(scale, low, max(low, width / 40)) if isfinite(scale) else low


def world_width(width, scale):
    return clamp(floor(width / scale), 40, 160)


def bounds(width):
    return (17, width - 17) if width >= 34 else (width // 2, width // 2)


EDGES = {
    "Stand": [("Sit", 0.6), ("Loaf", 0.9), ("Eat", 0.5), ("Crouch", 0.3), ("Stretch", 0.6)],
    "Sit": [("Stand", 0.6), ("Loaf", 0.7), ("Groom", 0.45), ("Yawn", 0.45), ("Happy", 0.4), ("Meow", 0.25)],
    "Loaf": [("Sit", 0.7), ("Stand", 0.9), ("Sleep", 0.8), ("Stretch", 0.7)],
    "Sleep": [("Loaf", 0.8)],
    "Eat": [("Stand", 0.5)],
    "Crouch": [("Stand", 0.3), ("Air", 0.2)],
    "Air": [("Crouch", 0.2)],
    "Stretch": [("Stand", 0.6), ("Loaf", 0.7)],
    **{name: [("Sit", 0.4)] for name in ("Groom", "Yawn", "Happy", "Meow")},
}


@lru_cache(maxsize=144)
def route(start, goal):
    queue = deque([(start, [])])
    seen = {start}
    while queue:
        node, path = queue.popleft()
        if node == goal:
            return path
        for target, duration in EDGES[node]:
            if target not in seen:
                seen.add(target)
                queue.append((target, [*path, (target, duration)]))
    raise ValueError(goal)


@dataclass(slots=True)
class Segment:
    start: float
    duration: float
    a: Pose
    b: Pose
    node: str
    x0: float
    x1: float
    f0: float
    f1: float
    anim: str
    lift: float
    linear: bool
    caption: str
    arc: float = 0


@dataclass(slots=True)
class Frame:
    pose: Pose
    core: Pose
    node: str
    x: float
    f: float
    anim: str
    caption: str
    lift: float = 0
    twist: float = 0
    bowl: tuple | None = None
    ball: tuple | None = None

    def origin(self):
        return {"n": NODES.index(self.node), "p": self.core.array(), "x": self.x, "f": self.f}


class Script:
    def __init__(self, width, origin=None):
        self.pose, self.node, self.x, self.f = origin or (KEYS["Sit"], "Sit", width // 2, 1)
        self.limits = bounds(width)
        self.segments = []
        self.t = 0
        self.caption = "坐著發呆"
        self.marks = {}
        self.ball = self.bowl = None

    def push(self, duration, pose, node, x, facing, anim="Still", lift=0, linear=False):
        duration = max(0.01, duration)
        self.segments.append(
            Segment(
                self.t, duration, self.pose, pose, node, self.x, x, self.f, facing, anim, lift, linear, self.caption
            )
        )
        self.t += duration
        self.pose, self.node, self.x, self.f = pose, node, x, facing

    def go(self, node):
        for target, duration in route(self.node, node):
            self.push(duration, KEYS[target], target, self.x, self.f)

    def hold(self, duration, anim):
        self.push(duration, self.pose, self.node, self.x, self.f, anim)

    def face(self, direction):
        if abs(self.f - direction) < 0.01 and direction != 0:
            return
        if self.node not in ("Sit", "Stand"):
            self.go("Sit" if direction == 0 else "Stand")
        self.push(0.5 + 0.55 * abs(self.f - direction), self.pose, self.node, self.x, direction, "Turn")

    def walk(self, x, run=False):
        if abs(self.x - x) < 0.5:
            return
        self.go("Stand")
        direction = copysign(1, x - self.x)
        speed, anim = (18, "Run") if run else (7, "Walk")
        lo, hi = self.limits
        radius = min(3.5 if run else 2.6, hi - self.x if self.f >= 0 else self.x - lo)
        if self.f * direction < 0 and radius >= 1.2:
            back = copysign(1, self.f)
            self.push(pi * radius / speed, self.pose, self.node, self.x, direction, anim, linear=True)
            self.segments[-1].arc = radius * back
        else:
            self.face(direction)
        self.push(abs(x - self.x) / speed, KEYS["Stand"], "Stand", x, self.f, anim, linear=True)

    def evaluate(self, elapsed, t):
        seg = next((s for s in reversed(self.segments) if s.start <= elapsed), self.segments[0])
        into = elapsed - seg.start
        u = clamp(into / seg.duration)
        e = u if seg.linear else smooth(u)
        base = mix(seg.a, seg.b, e)
        x = seg.x0 + (seg.x1 - seg.x0) * e + seg.arc * sin(pi * u)
        steps = max(2, floor(seg.duration * 2.6 + 0.5))
        turning = bool(seg.arc or seg.anim == "Turn")
        twist = 0
        if turning:
            progress = u if seg.arc else smooth(u - sin(2 * pi * steps * u) / (2 * pi * steps))
            a = (1 - seg.f0) * pi / 2
            delta = (seg.f0 - seg.f1) * pi / 2
            head = a + delta * clamp(1.65 * progress)
            hip = a + delta * clamp(1.65 * progress - 0.65)
            facing = 1 - (head + hip) / pi
            twist = head - hip
        else:
            facing = seg.f0 + (seg.f1 - seg.f0) * smooth(u)
        envelope = smooth(min(into, seg.duration - into) / 0.2)
        if (seg is self.segments[0] and into < 0.2 and seg.anim != "Still") or (
            seg is self.segments[-1] and seg.duration - into < 0.2 <= into
        ):
            envelope = 1
        distance = u * steps if seg.anim == "Turn" else u * pi * abs(seg.arc) if seg.arc else abs(x - seg.x0)
        core = mix(base, special(base, seg.anim, t, distance), envelope)
        return Frame(common(core, t), core, seg.node, x, facing, seg.anim, seg.caption, seg.lift * sin(pi * u), twist)


@lru_cache(maxsize=32)
def idle(width, start):
    sc = Script(width)
    seed = seed_at(start, 0x5EED)
    choice = (0, 1, 2, 3, 4, 1, 5, 2, 1, 3, 5, 0)[seed % 12]
    home = width // 2
    lo, hi = sc.limits
    if choice == 5:
        sc.hold(1 + ((seed >> 6) % 3), "Idle")
        sc.caption = "轉過來看你"
        sc.face(0)
        sc.hold(4 + ((seed >> 9) % 4), "Idle")
        sc.caption = "坐著發呆"
        sc.face(1)
    elif choice == 1:
        d = (14 + ((seed >> 8) % 24)) * (-1 if (seed >> 4) & 1 else 1)
        x = home + d if lo <= home + d <= hi else home - d
        sc.caption = "在散步"
        sc.walk(clamp(x, lo, hi))
        sc.caption = "坐下來看看"
        sc.go("Sit")
        sc.hold(2.5, "Idle")
        sc.caption = "走回來"
        sc.walk(home)
        sc.face(1)
        sc.caption = "坐著發呆"
        sc.go("Sit")
    elif choice == 2:
        sc.hold(1.5, "Idle")
        sc.caption = "在洗臉"
        sc.go("Groom")
        sc.hold(5, "Groom")
        sc.caption = "洗好臉了"
        sc.go("Sit")
    elif choice == 3:
        sc.hold(1.5, "Idle")
        sc.caption = "打了個呵欠"
        sc.go("Yawn")
        sc.hold(1.2, "Still")
        sc.go("Sit")
        sc.caption = "伸懶腰"
        sc.go("Stretch")
        sc.hold(1.6, "Still")
        sc.go("Stand")
        sc.caption = "坐著發呆"
        sc.go("Sit")
    elif choice == 4:
        sc.caption = "趴下來休息"
        sc.go("Loaf")
        sc.hold(9, "Idle")
        sc.go("Sit")
    sc.caption = "坐著發呆"
    if sc.t < 20:
        sc.hold(20 - sc.t, "Idle")
    elif sc.t > 20:
        for seg in sc.segments:
            seg.start *= 20 / sc.t
            seg.duration *= 20 / sc.t
        sc.t = 20
    return sc


def action(act, origin, width):
    sc = Script(width, origin)
    home = width // 2
    lo, hi = sc.limits
    x, node = sc.x, sc.node
    fs = -1 if sc.f < 0 else 1
    sc.caption = "轉過來看你"
    sc.push(0.4, KEYS[node], node, x, fs)
    if act == "eat":
        bx = min(home + 20, hi + 11, width - 5)
        direction = 1 if bx >= x else -1
        tx = clamp(bx - direction * 11, lo, hi)
        sc.bowl = bx
        sc.caption = "聞到飯香"
        sc.walk(tx, False)
        sc.face(direction)
        sc.caption = "正在吃飯"
        sc.go("Eat")
        sc.marks["eat0"] = sc.t
        sc.hold(6.5, "Eat")
        sc.marks["eat1"] = sc.t
        sc.caption = "吃飽了"
        sc.go("Stand")
        sc.caption = "在舔嘴巴"
        sc.go("Sit")
        sc.hold(2.4, "Lick")
    elif act == "pet":
        sc.caption = "被摸得呼嚕呼嚕"
        sc.go("Sit")
        sc.face(0)
        sc.go("Happy")
        sc.hold(3.5, "Happy")
        sc.go("Sit")
        sc.hold(0.6, "Idle")
        sc.face(1.0)
    elif act == "play":
        direction = 1 if x < width / 2 else -1
        bx = clamp(hi + 6 if direction > 0 else lo - 6, 4, width - 4)
        sc.ball = (x + direction * 4.0, bx)
        sc.caption = "看到球了！"
        sc.go("Stand")
        sc.face(direction)
        sc.go("Crouch")
        sc.hold(0.7, "Wiggle")
        sc.caption = "在追球"
        sc.go("Stand")
        sc.walk(bx - direction * 13.0, True)
        sc.go("Crouch")
        sc.hold(0.35, "Wiggle")
        sc.caption = "撲！"
        sc.go("Air")
        (px, ff) = (sc.x + direction * 6.0, sc.f)
        sc.push(0.45, KEYS["Air"], "Air", px, ff, "Still", 3.0, True)
        sc.go("Crouch")
        sc.go("Stand")
        sc.caption = "抓到球了"
        sc.go("Sit")
        sc.go("Happy")
        sc.hold(1.8, "Happy")
        sc.go("Sit")
    elif act == "look":
        sc.caption = "抬頭聽你說"
        sc.go("Sit")
        sc.face(0)
        tilt = replace(KEYS["Sit"])
        tilt.tilt += 0.35
        tilt.head = tilt.head + (0.3 + 0.4j)
        (x0, f0) = (sc.x, sc.f)
        sc.push(0.35, tilt, "Sit", x0, f0, "Still", 0.0, False)
        sc.push(1.4, tilt, "Sit", x0, f0, "Still", 0.0, False)
        sc.push(0.4, KEYS["Sit"], "Sit", x0, f0, "Still", 0.0, False)
        sc.hold(1.2, "Idle")
        sc.face(1.0)
    elif act == "paw":
        sc.caption = "邊看邊洗臉"
        sc.go("Sit")
        sc.go("Groom")
        sc.hold(1.6, "Groom")
        sc.go("Sit")
        sc.hold(0.4, "Idle")
    elif act == "call":
        sc.caption = "回了你一聲"
        sc.go("Sit")
        sc.face(0)
        sc.go("Meow")
        sc.hold(1.6, "Meow")
        sc.go("Sit")
        sc.hold(1.0, "Idle")
    elif act == "jump":
        direction = fs
        if x + direction * 10.0 > hi or x + direction * 10.0 < lo:
            direction = -direction
        sc.caption = "準備跳"
        sc.go("Stand")
        sc.face(direction)
        sc.go("Crouch")
        sc.hold(0.5, "Wiggle")
        sc.caption = "跳了起來"
        sc.go("Air")
        (jx, ff) = (clamp(sc.x + direction * 10, min(lo, sc.x), max(hi, sc.x)), sc.f)
        sc.push(0.7, KEYS["Air"], "Air", jx, ff, "Still", 3.1, True)
        sc.go("Crouch")
        sc.go("Stand")
        sc.caption = "落地"
        sc.go("Sit")
        sc.hold(0.8, "Idle")
    elif act == "nap":
        sc.caption = "被你哄睡了"
        sc.go("Sleep")
        sc.hold(20.0, "Sleep")
        sc.caption = "醒來了"
        sc.go("Loaf")
        sc.caption = "伸懶腰"
        sc.go("Stretch")
        sc.hold(1.4, "Still")
        sc.go("Stand")
        sc.go("Sit")
    elif act == "doze":
        sc.caption = "等到睡著了"
        sc.go("Sleep")
        sc.hold(4.0 * 3600.0, "Sleep")
    elif act == "wake":
        sc.caption = "醒來了"
        sc.go("Loaf")
        sc.caption = "伸懶腰"
        sc.go("Stretch")
        sc.hold(1.4, "Still")
        sc.go("Stand")
        sc.go("Sit")
        sc.caption = "跟你說好了"
        sc.face(0)
        sc.go("Meow")
        sc.hold(1.6, "Meow")
        sc.go("Sit")
    else:
        return None
    sc.caption = "走回原位"
    sc.walk(home, False)
    sc.face(1.0)
    sc.caption = "坐著發呆"
    sc.go("Sit")
    sc.hold(0.5, "Idle")
    return sc


@dataclass(slots=True)
class Playback:
    start: float
    width: float
    script: Script


def playback(data, width):
    try:
        start = float(data["t"])
        aw = float(data.get("w", 0))
        if not isfinite(start) or not isfinite(aw):
            return None
        if aw <= 0:
            aw = width
        origin = None
        try:
            fr = data["from"]
            x, f = float(fr["x"]), float(fr["f"])
            if 0 <= fr["n"] < len(NODES) and isfinite(x) and isfinite(f):
                origin = Pose.from_array(fr["p"]), NODES[fr["n"]], x, f
        except (TypeError, KeyError, ValueError, IndexError, OverflowError):
            pass
        script = action(data["act"], origin, aw)
        if script:
            return Playback(start, aw, script)
    except (AttributeError, KeyError, TypeError, ValueError, OverflowError):
        pass
    return None


@lru_cache(maxsize=8)
def _decode_pane(content, width):
    try:
        return playback(json.loads(content), width)
    except (UnicodeError, ValueError):
        return None


def read_pane(path, width):
    try:
        return _decode_pane(path.read_bytes(), width)
    except OSError:
        return None


def state(pane, t, width):
    anchor = 0.0
    if pane and t >= pane.start:
        sc = pane.script
        elapsed = t - pane.start
        if elapsed < sc.t:
            st = sc.evaluate(elapsed, t)
            lo, hi = bounds(width)
            old_lo, old_hi = bounds(pane.width)

            def fit(x):
                if pane.width == width:
                    return x
                if pane.width < width or old_hi <= old_lo:
                    return x - pane.width // 2 + width // 2
                return lo + (x - old_lo) * (hi - lo) / (old_hi - old_lo)

            st.x = clamp(fit(st.x), lo, hi)
            if sc.bowl is not None:
                a, b = sc.marks["eat0"], sc.marks["eat1"]
                st.bowl = clamp(fit(sc.bowl), 4, width - 4), clamp(1 - (elapsed - a) / (b - a))
            if sc.ball is not None:
                start, end = sc.ball
                u = min(elapsed / 1.6, 1)
                x = start + (end - start) * (1 - (1 - u) ** 2)
                st.ball = clamp(fit(x), 4, width - 4), 8 * (1 - u) ** 1.5 * abs(cos(u * 2.5 * pi))
            return st
        anchor = pane.start + sc.t
    start = anchor + max(0, floor((t - anchor) / 20)) * 20
    return idle(width, start).evaluate(t - start, t)


MESSAGES = {
    "eat": "你放了一碗飯，{} 走過去吃。",
    "pet": "你摸了摸 {}。",
    "play": "你丟了一顆球給 {}。",
    "call": "你叫了 {} 一聲。",
    "look": "{} 抬頭看你。",
    "paw": "{} 邊看邊洗臉。",
    "doze": "{} 睡著了。",
    "wake": "{} 醒來了。",
    "jump": "{} 跳！",
    "nap": "你哄 {} 睡覺。",
}


def act(what, cols, scale, name, path):
    if what not in MESSAGES:
        return "可以用：eat（吃飯）、pet（摸摸）、play（丟球）、call（叫牠）、jump（跳）、nap（哄睡）"
    cols = clamp(cols, 40, 400)
    width = world_width(cols, fit_scale(cols, scale))
    t = now()
    frame = state(read_pane(path, width), t, width)
    data = {"act": what, "t": t, "w": width, "from": frame.origin()}
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as tmp:
        try:
            json.dump(data, tmp)
            tmp.flush()
            os.replace(tmp.name, path)
        finally:
            Path(tmp.name).unlink(missing_ok=True)
    return MESSAGES[what].format(name)
