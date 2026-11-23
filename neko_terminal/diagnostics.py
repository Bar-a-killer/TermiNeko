"""Exercise interrupted animations at 5 ms intervals across every action."""

from cmath import exp

from .animation import Playback, action, state
from .poses import clamp, pose_values


def interrupt(what, pane, t, width):
    fr = state(pane, t, width)
    return Playback(t, width, action(what, (fr.core, fr.node, fr.x, fr.f), width))


def trace(cols):
    width = clamp(cols, 40, 160)
    acts = ("eat", "pet", "play", "call", "jump", "nap", "look", "paw", "wake", "doze")
    worst = 0.0
    for index, what in enumerate(acts):
        for k in range(12):
            trigger = 1763370000.0 + 20 * (index * 12 + k) + 1.7 * k
            prev = (
                interrupt(acts[(index + 1) % len(acts)], None, trigger - 2.3 - k * 0.4, width) if k % 3 == 2 else None
            )
            pane = interrupt(what, prev, trigger, width)
            last = None
            t = trigger - 1
            while t < trigger + 70:
                cur = state(prev if t < trigger else pane, t, width)
                if last is not None:
                    delta = [
                        a - b for a, b in zip(pose_values(last.pose)[:-2], pose_values(cur.pose)[:-2], strict=True)
                    ]
                    d = max(max(abs(z.real), abs(z.imag)) for z in delta)
                    dx, df = abs(last.x - cur.x), abs(last.f - cur.f)
                    score = max(d, dx / 1.2, df * 2)
                    worst = max(score, worst)
                    ear = cur.pose.head + (1 + 5.3j) * cur.pose.head_r / 3.2 * exp(1j * cur.pose.tilt)
                    top = cur.lift + 0.9 * (ear.imag + 0.6) + 1
                    if top > 21:
                        print(f"切耳 {what} k={k} t={t - trigger:+.2f} 高 {top:.1f} {cur.node}")
                    if score > 0.5:
                        print(
                            f"{what} k={k} t={t - trigger:+.2f} 跳 {score:.2f}（姿勢 {d:.2f} x {dx:.2f} 面向 {df:.2f}）"
                            f"{last.node}→{cur.node} {last.caption}→{cur.caption}"
                        )
                last = cur
                t += 0.005
    print(f"最大單幀變化 {worst:.2f}")
