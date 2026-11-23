"""Build the cat's rounded cones from a pose, with IK for the four legs."""

from cmath import exp
from math import dist, floor, sqrt

import numpy as np

from .poses import clamp

BODY, NECK, HEAD, MUZZLE, EAR, LEG, TAIL = range(7)
PRIMITIVE = np.dtype(
    [
        (name, "f8")
        for name in "ax ay az bx by bz r1 r2 zs blend side paw_x paw_y center_x center_y center_z radius".split()
    ]
    + [("part", "i4")]
)


def joint(a, b, upper, lower, bend):
    delta = b - a
    length = max(abs(delta), 1e-6)
    direction = delta * (1.0 / length)
    if length >= upper + lower - 1e-6:
        return a + direction * (length * upper / (upper + lower))
    along = (upper**2 - lower**2 + length**2) / (2 * length)
    normal = direction * 1j
    if normal.real * bend < 0:
        normal = -normal
    return a + along * direction + normal * sqrt(max(0, upper**2 - along**2))


def primitives(p):
    parts = []
    hk = p.head_r / 3.2
    rotation = exp(1j * p.tilt)

    def head(xy):
        return p.head + (xy * hk) * rotation

    def add(a, z0, b, z1, r1, r2, zs, blend, part, side=0, paw=0j):
        start = (a.real, a.imag, z0)
        end = (b.real, b.imag, z1)
        center = (a + b) / 2
        radius = dist(start, end) / 2 + max(r1, r2) / min(zs, 1)
        parts.append(
            (
                *start,
                *end,
                r1,
                r2,
                zs,
                blend,
                side,
                paw.real,
                paw.imag,
                center.real,
                center.imag,
                (z0 + z1) / 2,
                radius,
                part,
            )
        )

    add(p.hip, 0, p.chest, 0, p.hip_r, p.chest_r, 0.82, 0, BODY)
    add(p.chest, 0, p.head, 0, 2.1, 1.9, 0.9, 0.9, NECK)
    add(p.head, 0, p.head, 0, p.head_r, p.head_r, 1.12, 0.8, HEAD)
    cheek = head(1 - 0.9j)
    for side in (1, -1):
        add(cheek, side * 1.5 * hk, cheek - 0.6 + 0.2j, side * 1.7 * hk, 1.75 * hk, 1.5 * hk, 1, 0.9, HEAD)
    add(head(2.5 - 0.9j), 0, head(3.4 - 0.1j), 0, 1.45 * hk, 0.9 * hk, 0.85, 0.7, MUZZLE)
    add(head(2.2 + 1j), 0, head(3.4 - 0.1j), 0, 0.9 * hk, 0.7 * hk, 0.9, 0.7, MUZZLE)
    for side in (1, -1):
        add(
            head(0.75 + 2.4j), side * 1.55 * hk, head(1 + 5.3j), side * 2 * hk, 1.15 * hk, 0.18 * hk, 1, 0.25, EAR, side
        )
    for side, paw in ((1, p.fnp), (-1, p.ffp)):
        z, zs = side * 1.15, side * (1.15 - 0.25)
        s = clamp((paw.imag - 0.6) / 1.2)
        wrist = paw + complex(0.55 * s, 0.95 - 0.25 * s)
        shoulder = p.chest + 0.7 - 0.5j
        withers = p.chest + complex(-0.6, p.chest_r * 0.8)
        elbow = joint(shoulder, wrist, 3, 3.2, -1)
        add(withers, zs * 0.7, shoulder, zs, 1.15, 1.25, 1, 1, LEG, side, paw)
        add(shoulder, zs, elbow, z, 1.2, 0.85, 1, 0.7, LEG, side, paw)
        add(elbow, z, wrist, z, 0.74, 0.62, 1, 0.3, LEG, side, paw)
        add(wrist, z, paw, z, 0.7, 0.72, 1, 0.3, LEG, side, paw)
    for side, paw in ((1, p.bnp), (-1, p.bfp)):
        z = side * 1.45
        hock = paw + p.hock
        knee = joint(p.hip + 0.5 - 0.4j, hock, 3.3, 2.9, 1)
        haunch = p.hip + complex(0.2, p.hip_r * 0.45)
        add(haunch, z * 0.8, knee, z, 2.05, 1.05, 1, 1, LEG, side, paw)
        add(knee, z, hock, z, 0.78, 0.55, 1, 0.3, LEG, side, paw)
        add(hock, z, paw, z, 0.55, 0.66, 1, 0.3, LEG, side, paw)
    for z, paw in ((1.15, p.fnp), (-1.15, p.ffp), (1.45, p.bnp), (-1.45, p.bfp)):
        center = paw + 0.35 + 0.15j
        add(center - 0.3, z, center + 0.3, z, 0.85, 0.85, 1, 0.3, LEG, 0, paw)
    wrap = clamp((p.tail_t.real - p.hip.real) / 4) * 2.9
    base = p.hip + complex(-p.hip_r * 0.85, 0.4)
    curve = []
    for i in range(13):
        u = i / 12
        curve.append(base * (1 - u) ** 2 + p.tail_c * (2 * u * (1 - u)) + p.tail_t * (u * u))
    for i in range(8):
        a, b = i / 8, (i + 1) / 8
        add(
            curve[floor(a * 12 + 0.5)],
            wrap * a * a,
            curve[floor(b * 12 + 0.5)],
            wrap * b * b,
            1 - 0.28 * a,
            1 - 0.28 * b,
            1,
            0.6 if i == 0 else 0.05,
            TAIL,
            a,
        )
    return np.array(parts, dtype=PRIMITIVE)
