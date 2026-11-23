"""2-D skeleton in model units. Complex coordinates point forward and up."""

from dataclasses import astuple, dataclass, replace
from math import cos, floor, isfinite, pi, sin


def clamp(value, low=0.0, high=1.0):
    return max(low, min(value, high))


def smooth(u):
    u = clamp(u)
    return u * u * (3 - 2 * u)


@dataclass(slots=True)
class Pose:
    hip: complex
    hip_r: float
    chest: complex
    chest_r: float
    head: complex
    head_r: float
    tilt: float
    fnp: complex
    ffp: complex
    bnp: complex
    bfp: complex
    hock: complex
    tail_c: complex
    tail_t: complex
    eye: float = 0
    mouth: float = 0

    def array(self):
        values = []
        for value in astuple(self):
            if isinstance(value, complex):
                values.extend((value.real, value.imag))
            else:
                values.append(value)
        return values

    @classmethod
    def from_array(cls, a):
        if len(a) != 26 or not all(isinstance(v, (float, int)) and isfinite(v) for v in a):
            raise ValueError("invalid pane pose")
        return cls(
            complex(*a[:2]),
            a[2],
            complex(*a[3:5]),
            a[5],
            complex(*a[6:8]),
            a[8],
            a[9],
            *(complex(*a[i : i + 2]) for i in range(10, 24, 2)),
            a[24],
            a[25],
        )


def mix(a, b, u):
    if u <= 0:
        return replace(a)
    if u >= 1:
        return replace(b)
    p = Pose(*(x + (y - x) * u for x, y in zip(pose_values(a), pose_values(b), strict=True)))
    p.eye = a.eye if u < 0.5 else b.eye
    return p


def pose_values(p):
    return (
        p.hip,
        p.hip_r,
        p.chest,
        p.chest_r,
        p.head,
        p.head_r,
        p.tilt,
        p.fnp,
        p.ffp,
        p.bnp,
        p.bfp,
        p.hock,
        p.tail_c,
        p.tail_t,
        p.eye,
        p.mouth,
    )


NODES = ("Stand", "Sit", "Loaf", "Sleep", "Eat", "Crouch", "Air", "Stretch", "Groom", "Yawn", "Happy", "Meow")

SIT = Pose(
    (-3 + 4j),
    4.0,
    (1 + 9j),
    2.7,
    (2.4 + 13.9j),
    3.4,
    0.05,
    (3.9 + 0.6j),
    (2.8 + 0.6j),
    (0.9 + 0.6j),
    (-0.1 + 0.6j),
    (-4.2 + 0.3j),
    (-9.5 - 1.2j),
    (5.4 + 0.8j),
)
STAND = Pose(
    (-5 + 8.1j),
    2.75,
    (3.8 + 8.3j),
    2.85,
    (8.4 + 12.3j),
    3.4,
    0.0,
    (4.8 + 0.6j),
    (3.2 + 0.6j),
    (-5.4 + 0.6j),
    (-6.8 + 0.6j),
    (-1.4 + 2.6j),
    (-11 + 8j),
    (-11.5 + 14j),
)
LOAF = Pose(
    (-3.6 + 3.2j),
    3.1,
    (3 + 3.3j),
    3.0,
    (6.8 + 6.8j),
    3.4,
    0.0,
    (6 + 0.6j),
    (5 + 0.6j),
    (-1.2 + 0.6j),
    (-2.2 + 0.6j),
    (-3 + 0.2j),
    (-9 - 1j),
    (1.5 + 0.7j),
)
SLEEP = Pose(
    (-3.4 + 3.1j),
    3.2,
    (2.6 + 3j),
    3.0,
    (5.6 + 4j),
    3.4,
    -0.3,
    (5.4 + 0.6j),
    (4.4 + 0.6j),
    (-1.2 + 0.6j),
    (-2.2 + 0.6j),
    (-3 + 0.2j),
    (-9 - 1.5j),
    (6 + 1j),
    eye=1,
)
EAT = Pose(
    (-5 + 7.4j),
    3.0,
    (3.6 + 6.6j),
    3.0,
    (9.6 + 4.4j),
    3.4,
    -0.75,
    (5.4 + 0.6j),
    (3.8 + 0.6j),
    (-5.4 + 0.6j),
    (-6.8 + 0.6j),
    (-1.4 + 2.6j),
    (-11 + 9j),
    (-12.5 + 13j),
    eye=1,
)
CROUCH = Pose(
    (-4.6 + 5j),
    3.1,
    (3.6 + 4.6j),
    3.0,
    (8.2 + 7.8j),
    3.4,
    0.05,
    (6 + 0.6j),
    (4.6 + 0.6j),
    (-4 + 0.6j),
    (-5.2 + 0.6j),
    (-3.4 + 1.2j),
    (-10 + 4.5j),
    (-14 + 5.5j),
)
AIR = Pose(
    (-5.4 + 8.2j),
    3.0,
    (4.6 + 8.8j),
    3.0,
    (9.4 + 12.2j),
    3.4,
    0.15,
    (8.6 + 6.4j),
    (7.4 + 5.6j),
    (-10.6 + 4.6j),
    (-9.6 + 4j),
    (1.6 + 1.4j),
    (-11.5 + 9.5j),
    (-15 + 11j),
)
STRETCH = Pose(
    (-4.6 + 8.2j),
    3.0,
    (3.4 + 4j),
    2.8,
    (8.2 + 6.2j),
    3.4,
    0.3,
    (10 + 0.6j),
    (8.6 + 0.6j),
    (-5 + 0.6j),
    (-6.2 + 0.6j),
    (-0.8 + 3j),
    (-9 + 13j),
    (-6.5 + 16j),
    eye=1,
    mouth=0.8,
)
KEYS = dict(
    zip(
        NODES,
        (
            STAND,
            SIT,
            LOAF,
            SLEEP,
            EAT,
            CROUCH,
            AIR,
            STRETCH,
            replace(SIT, head=3.3 + 13.2j, tilt=-0.45, fnp=6.4 + 11.4j, eye=1),
            replace(SIT, head=2.4 + 13.8j, tilt=0.4, mouth=1, eye=1),
            replace(SIT, head=2.8 + 13.4j, tilt=0.15, eye=2),
            replace(SIT, tilt=0.3, mouth=0.7),
        ),
        strict=True,
    )
)


def special(p, anim, t, distance):
    p = replace(p)
    if anim in ("Walk", "Run"):
        run = anim == "Run"
        stride, duty, height = (11, 0.38, 1.6) if run else (7.5, 0.62, 1.25)
        cycle = distance / stride
        offsets = (0, 0.55, 0.1, 0.45) if run else (0, 0.25, 0.5, 0.75)
        for attr, offset in zip(("bnp", "fnp", "bfp", "ffp"), offsets, strict=True):
            phase = (cycle + offset) % 1
            reach = stride * duty / 2
            if phase < duty:
                delta = reach * (1 - 2 * phase / duty)
            else:
                swing = (phase - duty) / (1 - duty)
                front = attr[0] == "f"
                arc = sin(pi * swing)
                delta = complex(
                    -reach + 2 * reach * smooth(swing) - (0.5 * arc if front else 0),
                    height * arc**0.8 * (1 if front else 0.85),
                )
            setattr(p, attr, getattr(p, attr) + delta)
        w = cycle * 2 * pi
        if run:
            p.chest += complex(cos(w), 0.5 * sin(w - 0.9) - 0.5)
            p.hip += complex(-0.7 * cos(w), 0.5 * sin(w + 0.6) - 0.7)
            p.head += complex(0.8 * cos(w), 0.25 * sin(w - 0.9) - 0.8)
            p.tilt -= 0.1
            p.tail_t += complex(-2.5 + 0.6 * sin(w), -2.5)
        else:
            p.hip += 0.16j * cos(2 * w)
            p.chest += 1j * (0.28 * cos(2 * w + pi / 2) - 0.2)
            p.head += complex(-0.2, 0.06 * cos(2 * w + pi / 2) - 1.1)
            p.tilt -= 0.12
            p.tail_t += 0.7 * sin(w / 2)
            p.tail_c += 0.3j * sin(w)
    elif anim == "Turn":
        up = sin(pi * (distance % 1))
        even = floor(distance) % 2 == 0
        sitting = p.hip.imag < 5.5
        lift = (0.4 + 1.8j) * up
        if even:
            p.fnp += lift
            if not sitting:
                p.bfp += lift
        else:
            p.ffp += lift
            if not sitting:
                p.bnp += lift
        p.hip += 1j * up * (0.25 if sitting else 0.3)
        p.chest += 1j * up * (0.2 if sitting else 0.35)
        if not sitting:
            p.head += 0.2j * up
        p.tail_t += 0.8 * sin(distance * pi / 2)
    elif anim == "Sleep":
        p.hip_r += 0.15 * sin(t * 1.4)
        p.chest_r += 0.12 * sin(t * 1.4)
        p.eye = 1
    elif anim == "Eat":
        p.head += 0.45j * sin(t * 7)
        p.mouth = 0.5 if sin(t * 7) > 0 else 0
        p.eye = 1
    elif anim == "Groom":
        p.fnp += (0.5 + 0.6j) * sin(t * 6)
        p.tilt += 0.1 * sin(t * 6)
        p.mouth = 0.5 if sin(t * 6) > 0.2 else 0
        p.eye = 1
    elif anim == "Happy":
        p.head += 0.35 * sin(t * 3.5)
        p.tilt += 0.08 * sin(t * 3.5)
        p.eye = 2
        p.tail_t += 0.8 * sin(t * 5)
    elif anim == "Meow":
        p.mouth = 0.6 + 0.4 * abs(sin(t * 5))
    elif anim == "Lick":
        p.mouth = 0.5 if int(t * 6) % 2 == 0 else 0
        p.eye = 2
    elif anim == "Wiggle":
        p.hip += 0.45 * sin(t * 14)
        p.tail_t += 0.8j * sin(t * 9)
    elif anim == "Idle":
        look = sin(t * 0.45) * sin(t * 0.17 + 1)
        p.tilt += 0.18 * look
        p.head += 0.25 * look
    return p


def common(p, t):
    return replace(
        p,
        chest_r=p.chest_r + 0.08 * sin(t * 2.2),
        tail_t=p.tail_t + 0.9 * sin(t * 1.1),
        tail_c=p.tail_c + 0.4 * sin(t * 1.1 - 0.7),
        eye=1 if p.eye < 0.5 and max(0, int(t * 4)) % 23 == 0 else p.eye,
    )
