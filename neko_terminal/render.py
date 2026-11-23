"""Ray-march the articulated cat. Numba compiles the per-pixel numeric loops."""

from math import ceil, copysign, cos, floor, pi, sin, sqrt

import numpy as np
from numba import njit

from .geometry import BODY, EAR, HEAD, LEG, MUZZLE, NECK, TAIL, primitives


@njit(cache=True)
def cone(q, x, y, z):
    # Signed distance to a tapered capsule. Scale depth to flatten the torso.
    dx, dy, dz = q.bx - q.ax, q.by - q.ay, q.bz / q.zs - q.az / q.zs
    px, py, pz = x - q.ax, y - q.ay, z / q.zs - q.az / q.zs
    length2 = dx * dx + dy * dy + dz * dz
    if length2 < 1e-9:
        return (sqrt(px * px + py * py + pz * pz) - max(q.r1, q.r2)) * q.zs
    inverse = 1.0 / length2
    dr = q.r1 - q.r2
    a2 = length2 - dr * dr
    along = px * dx + py * dy + pz * dz
    end = along - length2
    cross2 = (px * length2 - dx * along) ** 2 + (py * length2 - dy * along) ** 2 + (pz * length2 - dz * along) ** 2
    y2, z2 = along * along * length2, end * end * length2
    limit = copysign(dr * dr, dr) * cross2
    if copysign(1.0, end) * a2 * z2 > limit:
        return (sqrt(cross2 + z2) * inverse - q.r2) * q.zs
    if copysign(1.0, along) * a2 * y2 < limit:
        return (sqrt(cross2 + y2) * inverse - q.r1) * q.zs
    return ((sqrt(max(0.0, cross2 * a2 * inverse)) + along * dr) * inverse - q.r1) * q.zs


@njit(cache=True)
def sdf(parts, x, y, z):
    distance, nearest, index = 1e9, 1e9, 0
    for i, q in enumerate(parts):
        bound = sqrt((x - q.center_x) ** 2 + (y - q.center_y) ** 2 + (z - q.center_z) ** 2) - q.radius
        if bound > distance + q.blend and bound > nearest:
            continue
        d = cone(q, x, y, z)
        if d < nearest:
            nearest, index = d, i
        h = max(0.0, q.blend - abs(distance - d)) / q.blend if q.blend else 0.0
        adjustment = h * h * q.blend * 0.25
        distance = min(distance, d) - adjustment
    return max(distance, -y), index


@njit(cache=True)
def normal(parts, x, y, z):
    nx = ny = nz = 0.0
    for a, b, c in ((1, -1, -1), (-1, -1, 1), (-1, 1, -1), (1, 1, 1)):
        d = sdf(parts, x + a * 0.04, y + b * 0.04, z + c * 0.04)[0]
        nx += a * d
        ny += b * d
        nz += c * d
    length = max(1e-9, sqrt(nx * nx + ny * ny + nz * nz))
    return nx / length, ny / length, nz / length


@njit(cache=True)
def face_material(x, y, z, part, eye, mouth, k):
    fine = k >= 1.9
    if fine and part == HEAD:
        length = max(1e-9, sqrt(x * x + y * y + (z / 1.12) ** 2))
        ux, uy, uz = x / length, y / length, z / (1.12 * length)
        for side in (1.0, -1.0):
            length = sqrt(0.7**2 + 0.2**2 + 0.62**2)
            ex, ey, ez = 0.7 / length, 0.2 / length, side * 0.62 / length
            if ux * ex + uy * ey + uz * ez < 0.8:
                continue
            length = sqrt(1 - ey * ey)
            tx, ty, tz = -ex * ey / length, (1 - ey * ey) / length, -ez * ey / length
            ax, ay, az = ux - ex, uy - ey, uz - ez
            dx = ax * (ey * tz - ez * ty) + ay * (ez * tx - ex * tz) + az * (ex * ty - ey * tx)
            dy = ax * tx + ay * ty + az * tz
            opening = min(1.0, max(0.0, 1 - 2 * eye))
            ry = 0.25 * opening
            if ry > 0.04:
                n = (dx / 0.2) ** 2 + (dy / ry) ** 2
                if n <= 1:
                    if k >= 3.2:
                        if (dx + 0.06 * side) ** 2 + (dy - ry * 0.45) ** 2 < 0.05**2:
                            return ord("w")
                        if n <= 0.72 and (dx / (0.06 + 0.04 * opening)) ** 2 + (dy / (ry * 0.85)) ** 2 > 1:
                            return ord("a")
                    return ord("e")
            else:
                xx = min(1.0, max(-1.0, dx / 0.22))
                curve = -0.02 + 0.07 * xx * xx if eye < 1.5 else 0.07 - 0.11 * xx * xx
                if abs(dx) <= 0.22 and abs(dy - curve) <= 0.035 + 0.25 / (k * 3.2):
                    return ord("e")
    if fine:
        if x > 3.3 and abs(z) < 0.42 and -0.55 + abs(z) * 0.8 < y < 0.15:
            return ord("n")
        if mouth > 0.3:
            m = (mouth - 0.3) / 0.7
            if x > 2.6 and -1 - 1.3 * m < y < -0.95 and abs(z) < 0.75:
                return ord("p") if y < -1 - 0.8 * m else ord("M")
        elif x > 2.8:
            lw = 0.12 + 0.3 / k
            if (abs(z) < lw * 0.8 and -1.15 < y < -0.6) or (
                0.05 < abs(z) < 0.9 and abs(y - (-1.3 + 1.4 * (abs(z) - 0.45) ** 2)) < lw
            ):
                return ord("m")
    if part == MUZZLE:
        return ord("o") if y > 0 and x < 3.4 else ord("w")
    if x > 0.7 and y < -0.9:
        return ord("w")
    if y > 1.6 and x > -1.5 and ((z * 1.4 + 10) / 1.2) % 1 < 0.4 and abs(z) < 1.8:
        return ord("d")
    return ord("o")


@njit(cache=True)
def material(p, q, x, y, z, k):
    if q.part == BODY or q.part == NECK:
        if q.part == BODY:
            ax, ay, bx, by = p[0], p[1], p[3], p[4]
        else:
            ax, ay, bx, by = p[3], p[4], p[6], p[7]
        length = max(1e-6, sqrt((bx - ax) ** 2 + (by - ay) ** 2))
        dx, dy = (bx - ax) / length, (by - ay) / length
        along = (x - ax) * dx + (y - ay) * dy
        u = min(1.0, max(0.0, along / length))
        offset = -(x - ax) * dy + (y - ay) * dx
        if q.part == NECK:
            return ord("w") if offset < -0.5 else ord("o")
        n = offset / (p[2] + (p[5] - p[2]) * u)
        if (n < -0.25 and u > 0.55) or n < -0.6:
            return ord("w")
        if n > -0.3 and 0.08 < u < 0.85 and (along / 2.4) % 1 < 0.38:
            return ord("d")
    elif q.part == LEG:
        if (x - q.paw_x) ** 2 + (y - q.paw_y) ** 2 < 1.4**2:
            return ord("w")
    elif q.part == TAIL:
        if q.side > 0.8 or (q.side * 7) % 1 < 0.32:
            return ord("d")
    else:
        hk = p[8] / 3.2
        dx, dy = (x - p[6]) / hk, (y - p[7]) / hk
        xx, yy, zz = dx * cos(p[9]) + dy * sin(p[9]), -dx * sin(p[9]) + dy * cos(p[9]), z / hk
        if q.part != EAR:
            return face_material(xx, yy, zz, q.part, p[24], p[25], k)
        ex, ey, ez = xx - 0.75, yy - 2.4, zz - q.side * 1.55
        u = min(1.0, max(0.0, (ex * 0.25 + ey * 2.9 + ez * q.side * 0.45) / (0.25**2 + 2.9**2 + 0.45**2)))
        radius = 1.15 + (0.18 - 1.15) * u
        if (
            ex - 0.25 * u > 0.1
            and 0.1 < u < 0.85
            and (ey - 2.9 * u) ** 2 + (ez - q.side * 0.45 * u) ** 2 < (radius * 0.6) ** 2
        ):
            return ord("p")
    return ord("o")


@njit(cache=True)
def angle(x, th, twist, lo, span):
    return th + twist * (min(1.0, max(0.0, (x - lo) / span)) - 0.5)


@njit(cache=True)
def spine_integral(x, th, twist, lo, hi, span):
    # Antiderivative of the spine direction measured from its hip end.
    a = th - twist / 2
    if x <= lo:
        return (x - lo) * cos(a), (x - lo) * sin(a)
    b = angle(min(x, hi), th, twist, lo, span)
    length = min(x, hi) - lo
    if abs(b - a) < 1e-9:
        sx, sz = length * cos(a), length * sin(a)
    else:
        sx, sz = (sin(b) - sin(a)) * length / (b - a), (cos(a) - cos(b)) * length / (b - a)
    extra = max(0.0, x - hi)
    return sx + extra * cos(b), sz + extra * sin(b)


@njit(cache=True)
def view(x, z, params):
    th, twist, lo, hi, span, ox, oz = params
    if twist == 0:
        return x * cos(th) - z * sin(th), x * sin(th) + z * cos(th)
    sx, sz = spine_integral(x, th, twist, lo, hi, span)
    a = angle(x, th, twist, lo, span)
    return sx - ox - z * sin(a), sz - oz + z * cos(a)


@njit(cache=True)
def to_model(x, z, params):
    th, twist, lo, hi, span, ox, oz = params
    xx, zz = x * cos(th) + z * sin(th), -x * sin(th) + z * cos(th)
    if twist != 0:
        for _ in range(5):
            sx, sz = spine_integral(xx, th, twist, lo, hi, span)
            a = angle(xx, th, twist, lo, span)
            dx, dz = x - sx + ox, z - sz + oz
            xx += dx * cos(a) + dz * sin(a)
            zz = -dx * sin(a) + dz * cos(a)
    return xx, zz


@njit(cache=True)
def shade(base, tone):
    if base == ord("o"):
        return (108, 111, 115, 116)[tone]  # l o s t
    if base == ord("d"):
        return (103, 100, 68, 68)[tone]  # g d D D
    if base == ord("w"):
        return (119, 119, 87, 86)[tone]  # w w W V
    if base == ord("p") and tone >= 2:
        return ord("P")
    return base


@njit(cache=True)
def ray(parts, p, k, x, y, z0, z1, params):
    z = z1
    for _ in range(64):
        if z <= z0:
            break
        mx, mz = to_model(x, z, params)
        d, index = sdf(parts, mx, y, mz)
        if d < 0.015:
            base = material(p, parts[index], mx, y, mz, k)
            nx, ny, nz = normal(parts, mx, y, mz)
            a = angle(mx, params[0], params[1], params[2], params[4])
            vx, vz = nx * cos(a) - nz * sin(a), nx * sin(a) + nz * cos(a)
            ao = 0.0
            for h in (0.5, 1.1):
                ao += max(0.0, h - sdf(parts, mx + nx * h, y + ny * h, mz + nz * h)[0]) / h
            light = (-0.45 * vx + 0.8 * ny + 0.55 * vz) / sqrt(0.45**2 + 0.8**2 + 0.55**2)
            lum = 0.3 + 0.75 * max(0.0, light) + 0.15 * max(0.0, vz) - 0.35 * ao
            tone = 0 if lum > 0.92 else 1 if lum > 0.55 else 2 if lum > 0.32 else 3
            return shade(base, tone), z
        z -= max(0.01, d) * (0.75 if params[1] else 0.9)
    return 32, -np.inf


@njit(cache=True)
def draw(parts, p, w, h, k, cx, f, lift, twist):
    th = (1 - min(1.0, max(-1.0, f))) * pi / 2
    lo, hi = min(p[0], p[6]), max(p[0], p[6])
    span = max(2.0, hi - lo)
    ox, oz = spine_integral(0.0, th, twist, lo, hi, span)
    params = (th, twist, lo, hi, span, ox, oz)
    px, ground, s = cx * k, h - 1 - lift * k, 0.9 * k
    x0, z0, x1, z1, top = 1e9, 1e9, -1e9, -1e9, -1e9
    for q in parts:
        r = max(q.r1, q.r2) / min(q.zs, 1.0) + 0.6
        top = max(top, max(q.ay, q.by) + max(q.r1, q.r2) + 0.6)
        for x, z in ((q.ax, q.az), (q.bx, q.bz)):
            vx, vz = view(x, z, params)
            x0, x1, z0, z1 = min(x0, vx - r), max(x1, vx + r), min(z0, vz - r), max(z1, vz + r)
    left, right = max(0, floor(px + x0 * s)), min(w, ceil(px + x1 * s))
    first = max(0, floor(ground - top * s))
    layer = np.full((h, w), 32, np.uint8)
    depth = np.full((h, w), -np.inf)
    hits = np.empty(4, np.int32)
    samples = 4 if k < 1.9 else 1
    for j in range(first, h):
        for i in range(left, right):
            n = 0
            best_depth = -np.inf
            for sample in range(samples):
                sx = 0.5 if samples == 1 else 0.25 + 0.5 * (sample % 2)
                sy = 0.5 if samples == 1 else 0.25 + 0.5 * (sample // 2)
                y = (ground - j - sy) / s
                if y < -0.05:
                    continue
                color, z = ray(parts, p, k, (i + sx - px) / s, y, z0, z1, params)
                if color != 32:
                    hits[n] = color
                    n += 1
                    best_depth = max(best_depth, z)
            if n < (2 if samples == 4 else 1):
                continue
            best_count = 0
            for a in range(n):
                count = 0
                for b in range(n):
                    count += hits[a] == hits[b]
                if count > best_count:
                    best_count = count
                    layer[j, i] = hits[a]
            depth[j, i] = best_depth
    radius = max(1, floor(k * 0.55 + 0.5))
    for j in range(max(0, first - radius), h):
        for i in range(max(0, left - radius), min(w, right + radius)):
            here = depth[j, i]
            outlined = False
            for dy in range(-radius, radius + 1):
                yy = j + dy
                if not 0 <= yy < h:
                    continue
                reach = radius - abs(dy)
                for xx in range(max(0, i - reach), min(w, i + reach + 1)):
                    if depth[yy, xx] > here + 1.6:
                        layer[j, i] = ord("k") if here == -np.inf else ord("K")
                        outlined = True
                        break
                if outlined:
                    break
    hk = p[8] / 3.2
    if k < 1.9:
        for index, (x, y, z) in enumerate(((3.75, -0.3, 0.0), (3.4, -1.3, 0.0), (1.7, 0.6, 1.55), (1.7, 0.6, -1.55))):
            mx = p[6] + hk * (x * cos(p[9]) - y * sin(p[9]))
            my = p[7] + hk * (x * sin(p[9]) + y * cos(p[9]))
            vx, vz = view(mx, z * hk, params)
            i, j = floor(px + vx * s), floor(ground - my * s)
            if not (0 <= i < w and 0 <= j < h) or layer[j, i] in (32, 107):
                continue
            if index < 2:
                if abs(depth[j, i] - vz) < 1.2 and (index == 0 or p[25] > 0.35):
                    layer[j, i] = ord("n") if index == 0 else ord("p")
            else:
                a = angle(p[6], th, twist, lo, span)
                if 0.7 * sin(a) + copysign(0.62, z) * cos(a) < 0.05:
                    continue
                layer[j, i] = ord("e")
                if p[24] < 0.5:
                    if j > 0:
                        layer[j - 1, i] = ord("e")
                else:
                    center = view(p[6] - hk * 0.6 * sin(p[9]), 0.0, params)[0]
                    side = i + (1 if i > px + center * s else -1)
                    if 0 <= side < w:
                        layer[j, side] = ord("e")
    return layer, px + view(p[6], 0.0, params)[0] * s, ground - p[7] * s


def render(pose, w, h, scale, x, facing, lift=0.0, twist=0.0):
    return draw(
        primitives(pose),
        np.asarray(pose.array(), dtype=float),
        w,
        h,
        float(scale),
        float(x),
        float(facing),
        float(lift),
        float(twist),
    )
