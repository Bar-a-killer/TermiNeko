//! 點陣圖版的貓（給 Claude Code 用）。
//!
//! 貓不是一張張手畫的圖，而是用膠囊、圓、三角形組成的「骨架」：
//! 臀部、胸口、頭、四隻腳、尾巴各有位置。每個姿勢是一組數字，
//! 換姿勢時把數字慢慢插值過去，所以任何兩個行為之間都有過渡動畫。
//!
//! 畫面完全由「時間＋pane.json」算出來，不需要常駐程式：
//! - 平常：每 20 秒一段閒晃劇本（坐、散步、洗臉、伸懶腰、趴下、打盹…），
//!   每段都從「坐在中間」開始、回到「坐在中間」結束，所以段與段之間是連續的。
//! - 按下動作：記下當下的姿勢，動作劇本從那個姿勢接過去，做完再走回中間坐好。

use serde_json::{json, Value};
use std::f64::consts::PI;
use std::ops::{Add, Mul, Sub};
use std::path::Path;

pub const ROWS: usize = 11;
pub(crate) const HPX: usize = ROWS * 2; // 每格上下兩個像素
const S: f64 = 0.9; // 姿勢座標 → 像素
pub(crate) const NONE: u32 = 0x0100_0000;
const SLOT: f64 = 20.0;

pub(crate) fn now() -> f64 {
    if let Some(t) = std::env::var("NEKO_NOW").ok().and_then(|s| s.parse().ok()) {
        return t;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub(crate) fn hash(n: u64) -> u64 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 32)
}

// ───────────────────────── 向量 ─────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct V {
    x: f64,
    y: f64,
}
const fn v(x: f64, y: f64) -> V {
    V { x, y }
}
impl Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        v(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        v(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for V {
    type Output = V;
    fn mul(self, k: f64) -> V {
        v(self.x * k, self.y * k)
    }
}
impl V {
    fn rot(self, a: f64) -> V {
        let (s, c) = a.sin_cos();
        v(self.x * c - self.y * s, self.x * s + self.y * c)
    }
    fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    fn dot(self, o: V) -> f64 {
        self.x * o.x + self.y * o.y
    }
}
fn lerp(a: f64, b: f64, u: f64) -> f64 {
    a + (b - a) * u
}
fn smooth(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

// ───────────────────────── 姿勢 ─────────────────────────
// 座標：x 向前（貓面向右）、y 向上，原點是身體正下方的地面。單位約等於 1/S 像素。

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pose {
    hip: V,
    hip_r: f64,
    chest: V,
    chest_r: f64,
    head: V,
    head_r: f64,
    tilt: f64,
    fnp: V,  // 前腳（近）腳掌
    ffp: V,  // 前腳（遠）
    bnp: V,  // 後腳（近）
    bfp: V,  // 後腳（遠）
    hock: V, // 後腳跗關節相對於腳掌的位置
    tail_c: V,
    tail_t: V,
    eye: f64, // 0 張開、1 閉著、2 開心
    mouth: f64,
}

const N: usize = 26;

impl Pose {
    fn arr(&self) -> [f64; N] {
        [
            self.hip.x,
            self.hip.y,
            self.hip_r,
            self.chest.x,
            self.chest.y,
            self.chest_r,
            self.head.x,
            self.head.y,
            self.head_r,
            self.tilt,
            self.fnp.x,
            self.fnp.y,
            self.ffp.x,
            self.ffp.y,
            self.bnp.x,
            self.bnp.y,
            self.bfp.x,
            self.bfp.y,
            self.hock.x,
            self.hock.y,
            self.tail_c.x,
            self.tail_c.y,
            self.tail_t.x,
            self.tail_t.y,
            self.eye,
            self.mouth,
        ]
    }
    fn from(a: &[f64]) -> Option<Pose> {
        if a.len() != N || a.iter().any(|x| !x.is_finite()) {
            return None;
        }
        Some(Pose {
            hip: v(a[0], a[1]),
            hip_r: a[2],
            chest: v(a[3], a[4]),
            chest_r: a[5],
            head: v(a[6], a[7]),
            head_r: a[8],
            tilt: a[9],
            fnp: v(a[10], a[11]),
            ffp: v(a[12], a[13]),
            bnp: v(a[14], a[15]),
            bfp: v(a[16], a[17]),
            hock: v(a[18], a[19]),
            tail_c: v(a[20], a[21]),
            tail_t: v(a[22], a[23]),
            eye: a[24],
            mouth: a[25],
        })
    }
    fn mix(a: &Pose, b: &Pose, u: f64) -> Pose {
        let (x, y) = (a.arr(), b.arr());
        let mut z = [0.0; N];
        for i in 0..N {
            z[i] = lerp(x[i], y[i], u);
        }
        z[24] = if u < 0.5 { x[24] } else { y[24] };
        Pose::from(&z).unwrap_or(*b)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum P {
    Stand,
    Sit,
    Loaf,
    Sleep,
    Eat,
    Crouch,
    Air,
    Stretch,
    Groom,
    Yawn,
    Happy,
    Meow,
}
const ALL: [P; 12] = [
    P::Stand,
    P::Sit,
    P::Loaf,
    P::Sleep,
    P::Eat,
    P::Crouch,
    P::Air,
    P::Stretch,
    P::Groom,
    P::Yawn,
    P::Happy,
    P::Meow,
];

fn idx(p: P) -> usize {
    ALL.iter().position(|q| *q == p).unwrap_or(1)
}

#[allow(clippy::too_many_arguments)]
fn mk(
    hip: (f64, f64),
    hr: f64,
    chest: (f64, f64),
    cr: f64,
    head: (f64, f64),
    tilt: f64,
    fnp: (f64, f64),
    ffp: (f64, f64),
    bnp: (f64, f64),
    bfp: (f64, f64),
    hock: (f64, f64),
    tc: (f64, f64),
    tt: (f64, f64),
) -> Pose {
    let p = |a: (f64, f64)| v(a.0, a.1);
    Pose {
        hip: p(hip),
        hip_r: hr,
        chest: p(chest),
        chest_r: cr,
        head: p(head),
        head_r: 3.4,
        tilt,
        fnp: p(fnp),
        ffp: p(ffp),
        bnp: p(bnp),
        bfp: p(bfp),
        hock: p(hock),
        tail_c: p(tc),
        tail_t: p(tt),
        eye: 0.0,
        mouth: 0.0,
    }
}

/// 各姿勢的骨架。比例參考真的貓：身長約肩高的 1.4 倍、腹下空間約肩高四成、頭寬約等於身體厚度。
pub(crate) fn key(p: P) -> Pose {
    let sit = mk(
        (-3.0, 4.0),
        4.0,
        (1.0, 9.0),
        2.7,
        (2.4, 13.9),
        0.05,
        (3.9, 0.6),
        (2.8, 0.6),
        (0.9, 0.6),
        (-0.1, 0.6),
        (-4.2, 0.3),
        (-9.5, -1.2),
        (5.4, 0.8),
    );
    match p {
        P::Stand => mk(
            (-5.0, 8.1),
            2.75,
            (3.8, 8.3),
            2.85,
            (8.4, 12.3),
            0.0,
            (4.8, 0.6),
            (3.2, 0.6),
            (-5.4, 0.6),
            (-6.8, 0.6),
            (-1.4, 2.6),
            (-11.0, 8.0),
            (-11.5, 14.0),
        ),
        P::Sit => sit,
        P::Loaf => mk(
            (-3.6, 3.2),
            3.1,
            (3.0, 3.3),
            3.0,
            (6.8, 6.8),
            0.0,
            (6.0, 0.6),
            (5.0, 0.6),
            (-1.2, 0.6),
            (-2.2, 0.6),
            (-3.0, 0.2),
            (-9.0, -1.0),
            (1.5, 0.7),
        ),
        P::Sleep => {
            let mut s = mk(
                (-3.4, 3.1),
                3.2,
                (2.6, 3.0),
                3.0,
                (5.6, 4.0),
                -0.3,
                (5.4, 0.6),
                (4.4, 0.6),
                (-1.2, 0.6),
                (-2.2, 0.6),
                (-3.0, 0.2),
                (-9.0, -1.5),
                (6.0, 1.0),
            );
            s.eye = 1.0;
            s
        }
        P::Eat => {
            let mut s = mk(
                (-5.0, 7.4),
                3.0,
                (3.6, 6.6),
                3.0,
                (9.6, 4.4),
                -0.75,
                (5.4, 0.6),
                (3.8, 0.6),
                (-5.4, 0.6),
                (-6.8, 0.6),
                (-1.4, 2.6),
                (-11.0, 9.0),
                (-12.5, 13.0),
            );
            s.eye = 1.0;
            s
        }
        P::Crouch => mk(
            (-4.6, 5.0),
            3.1,
            (3.6, 4.6),
            3.0,
            (8.2, 7.8),
            0.05,
            (6.0, 0.6),
            (4.6, 0.6),
            (-4.0, 0.6),
            (-5.2, 0.6),
            (-3.4, 1.2),
            (-10.0, 4.5),
            (-14.0, 5.5),
        ),
        P::Air => mk(
            (-5.4, 8.2),
            3.0,
            (4.6, 8.8),
            3.0,
            (9.4, 12.2),
            0.15,
            (8.6, 6.4),
            (7.4, 5.6),
            (-10.6, 4.6),
            (-9.6, 4.0),
            (1.6, 1.4),
            (-11.5, 9.5),
            (-15.0, 11.0),
        ),
        P::Stretch => {
            let mut s = mk(
                (-4.6, 8.2),
                3.0,
                (3.4, 4.0),
                2.8,
                (8.2, 6.2),
                0.3,
                (10.0, 0.6),
                (8.6, 0.6),
                (-5.0, 0.6),
                (-6.2, 0.6),
                (-0.8, 3.0),
                (-9.0, 13.0),
                (-6.5, 16.0),
            );
            s.mouth = 0.8;
            s.eye = 1.0;
            s
        }
        P::Groom => {
            let mut s = sit;
            s.head = v(3.3, 13.2);
            s.tilt = -0.45;
            s.fnp = v(6.4, 11.4);
            s.eye = 1.0;
            s
        }
        P::Yawn => {
            let mut s = sit;
            s.head = v(2.4, 13.8);
            s.tilt = 0.4;
            s.mouth = 1.0;
            s.eye = 1.0;
            s
        }
        P::Happy => {
            let mut s = sit;
            s.head = v(2.8, 13.4);
            s.tilt = 0.15;
            s.eye = 2.0;
            s
        }
        P::Meow => {
            let mut s = sit;
            s.tilt = 0.3;
            s.mouth = 0.7;
            s
        }
    }
}

/// 姿勢之間能直接轉換的路（秒數是過渡動畫的長度）
fn edges(p: P) -> &'static [(P, f64)] {
    match p {
        P::Stand => &[(P::Sit, 0.6), (P::Loaf, 0.9), (P::Eat, 0.5), (P::Crouch, 0.3), (P::Stretch, 0.6)],
        P::Sit => &[
            (P::Stand, 0.6),
            (P::Loaf, 0.7),
            (P::Groom, 0.45),
            (P::Yawn, 0.45),
            (P::Happy, 0.4),
            (P::Meow, 0.25),
        ],
        P::Loaf => &[(P::Sit, 0.7), (P::Stand, 0.9), (P::Sleep, 0.8), (P::Stretch, 0.7)],
        P::Sleep => &[(P::Loaf, 0.8)],
        P::Eat => &[(P::Stand, 0.5)],
        P::Crouch => &[(P::Stand, 0.3), (P::Air, 0.2)],
        P::Air => &[(P::Crouch, 0.2)],
        P::Stretch => &[(P::Stand, 0.6), (P::Loaf, 0.7)],
        P::Groom | P::Yawn | P::Happy | P::Meow => &[(P::Sit, 0.4)],
    }
}

fn route(from: P, to: P) -> Vec<(P, f64)> {
    let mut prev: [Option<(P, f64)>; 12] = [None; 12];
    let mut seen = [false; 12];
    let mut q = std::collections::VecDeque::from([from]);
    seen[idx(from)] = true;
    while let Some(p) = q.pop_front() {
        if p == to {
            break;
        }
        for &(n, d) in edges(p) {
            if !seen[idx(n)] {
                seen[idx(n)] = true;
                prev[idx(n)] = Some((p, d));
                q.push_back(n);
            }
        }
    }
    let mut path = vec![];
    let mut cur = to;
    while cur != from {
        match prev[idx(cur)] {
            Some((p, d)) => {
                path.push((cur, d));
                cur = p;
            }
            None => return vec![],
        }
    }
    path.reverse();
    path
}

// ───────────────────────── 劇本 ─────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Anim {
    Idle,
    Still,
    Walk,
    Run,
    Eat,
    Groom,
    Happy,
    Sleep,
    Meow,
    Lick,
    Wiggle,
    Turn, // 原地轉身：腳一步一步踩
}

#[derive(Clone)]
struct Seg {
    t0: f64,
    dur: f64,
    a: Pose,
    b: Pose,
    node: P,
    x0: f64,
    x1: f64,
    f0: f64,
    f1: f64,
    anim: Anim,
    lift: f64,
    linear: bool,
    cap: &'static str,
    arc: f64, // 迴轉：先往原本的方向多走出去這麼遠再繞回來（0 是直線）
}

struct Sc {
    segs: Vec<Seg>,
    pose: Pose,
    node: P,
    x: f64,
    f: f64,
    t: f64,
    cap: &'static str,
    marks: Vec<(&'static str, f64)>,
    bowl: Option<f64>,
    ball: Option<(f64, f64)>,
    lim: (f64, f64), // 貓的位置不能超出這個範圍（迴轉用）
}

pub(crate) struct St {
    pub(crate) pose: Pose,
    pub(crate) core: Pose,
    pub(crate) x: f64,
    pub(crate) f: f64,
    pub(crate) lift: f64,
    pub(crate) twist: f64, // 轉身時頭比屁股先轉多少（弧度）
    pub(crate) anim: Anim,
    pub(crate) node: P,
    pub(crate) cap: &'static str,
}

impl Sc {
    fn new(pose: Pose, node: P, x: f64, f: f64) -> Sc {
        Sc {
            segs: vec![],
            pose,
            node,
            x,
            f,
            t: 0.0,
            cap: "坐著發呆",
            marks: vec![],
            bowl: None,
            ball: None,
            lim: (-1e9, 1e9),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, dur: f64, b: Pose, node: P, x1: f64, f1: f64, anim: Anim, lift: f64, linear: bool) {
        let dur = dur.max(0.01);
        self.segs.push(Seg {
            t0: self.t,
            dur,
            a: self.pose,
            b,
            node,
            x0: self.x,
            x1,
            f0: self.f,
            f1,
            anim,
            lift,
            linear,
            cap: self.cap,
            arc: 0.0,
        });
        self.t += dur;
        self.pose = b;
        self.node = node;
        self.x = x1;
        self.f = f1;
    }
    fn say(&mut self, cap: &'static str) {
        self.cap = cap;
    }
    fn mark(&mut self, name: &'static str) {
        self.marks.push((name, self.t));
    }
    fn at(&self, name: &str) -> f64 {
        self.marks.iter().find(|m| m.0 == name).map(|m| m.1).unwrap_or(f64::MAX)
    }
    fn go(&mut self, to: P) {
        for (p, d) in route(self.node, to) {
            self.push(d, key(p), p, self.x, self.f, Anim::Still, 0.0, false);
        }
    }
    fn hold(&mut self, dur: f64, anim: Anim) {
        let (p, n, x, f) = (self.pose, self.node, self.x, self.f);
        self.push(dur, p, n, x, f, anim, 0.0, false);
    }
    fn face(&mut self, dir: f64) {
        if (self.f - dir).abs() < 0.01 {
            return;
        }
        if self.node != P::Sit && self.node != P::Stand {
            self.go(P::Stand);
        }
        let (p, n, x) = (self.pose, self.node, self.x);
        let dur = 0.5 + 0.55 * (self.f - dir).abs();
        self.push(dur, p, n, x, dir, Anim::Turn, 0.0, false);
    }
    /// 轉過來面對你（坐著或站著轉身）
    fn face_you(&mut self) {
        if self.node != P::Sit && self.node != P::Stand {
            self.go(P::Sit);
        }
        let (p, n, x) = (self.pose, self.node, self.x);
        let dur = 0.5 + 0.55 * self.f.abs();
        self.push(dur, p, n, x, 0.0, Anim::Turn, 0.0, false);
    }
    fn walk(&mut self, x: f64, run: bool) {
        if (x - self.x).abs() < 0.5 {
            return;
        }
        self.go(P::Stand);
        let dir = if x > self.x { 1.0 } else { -1.0 };
        let speed = if run { 18.0 } else { 7.0 };
        let anim = if run { Anim::Run } else { Anim::Walk };
        // 迴轉會先往原本的方向多走出去；靠邊的時候空間不夠就原地轉
        let back = self.f.signum();
        let room = if back > 0.0 { self.lim.1 - self.x } else { self.x - self.lim.0 };
        let r = (if run { 3.5f64 } else { 2.6 }).min(room);
        if self.f * dir < 0.0 && r >= 1.2 {
            // 要往反方向走：像真貓一樣邊走邊繞一個小 U 字迴轉，不是原地轉
            let (p, n, x0) = (self.pose, self.node, self.x);
            self.push(PI * r / speed, p, n, x0, dir, anim, 0.0, true);
            self.segs.last_mut().unwrap().arc = r * back;
        } else {
            self.face(dir);
        }
        let f = self.f;
        self.push(
            (x - self.x).abs() / speed,
            key(P::Stand),
            P::Stand,
            x,
            f,
            if run { Anim::Run } else { Anim::Walk },
            0.0,
            true,
        );
    }
    /// 補滿到固定長度；太長就整段等比例加速
    fn fill(&mut self, total: f64) {
        if self.t < total {
            let a = if self.node == P::Sleep { Anim::Sleep } else { Anim::Idle };
            self.hold(total - self.t, a);
        } else if self.t > total {
            let k = total / self.t;
            for s in &mut self.segs {
                s.t0 *= k;
                s.dur *= k;
            }
            self.t = total;
        }
    }
    fn eval(&self, tr: f64, tg: f64) -> St {
        let seg = match self.segs.iter().rev().find(|s| s.t0 <= tr).or(self.segs.first()) {
            Some(s) => s,
            None => {
                return St {
                    pose: common(self.pose, tg),
                    core: self.pose,
                    x: self.x,
                    f: self.f,
                    lift: 0.0,
                    twist: 0.0,
                    anim: Anim::Idle,
                    node: self.node,
                    cap: self.cap,
                }
            }
        };
        let u = ((tr - seg.t0) / seg.dur).clamp(0.0, 1.0);
        let e = if seg.linear { u } else { smooth(u) };
        let base = Pose::mix(&seg.a, &seg.b, e);
        // 迴轉：路線是往外走再繞回來的半圓（從側面看就是先往前、再往回）
        let x = lerp(seg.x0, seg.x1, e) + seg.arc * (PI * u).sin();
        // 轉身：一步轉一點，腳落地時停一下（像樓梯），步數跟時間成正比
        let steps = (seg.dur * 2.6).round().max(2.0);
        // 迴轉和原地轉身：頭先轉、屁股慢一拍（lead 越大差越多），面向取兩者中間
        let turning = seg.arc != 0.0 || seg.anim == Anim::Turn;
        let (th0, th1) = ((1.0 - seg.f0) * PI / 2.0, (1.0 - seg.f1) * PI / 2.0);
        let lead = 0.65;
        // 原地轉身一步轉一點（腳落地時停一下）
        let ue = if seg.arc != 0.0 {
            u
        } else {
            smooth(u - (2.0 * PI * steps * u).sin() / (2.0 * PI * steps))
        };
        let th_head = lerp(th0, th1, ((1.0 + lead) * ue).clamp(0.0, 1.0));
        let th_hip = lerp(th0, th1, ((1.0 + lead) * ue - lead).clamp(0.0, 1.0));
        let f = if turning {
            1.0 - (th_head + th_hip) / PI
        } else if seg.anim == Anim::Turn {
            let s = u - (2.0 * PI * steps * u).sin() / (2.0 * PI * steps);
            lerp(seg.f0, seg.f1, smooth(s))
        } else {
            lerp(seg.f0, seg.f1, smooth(u))
        };
        let lift = seg.lift * (PI * u).sin();
        // 這一段特有的小動作在頭尾 0.2 秒淡入淡出，段與段之間才不會跳
        let into = tr - seg.t0;
        let env = smooth((into / 0.2).min((seg.dur - into) / 0.2).clamp(0.0, 1.0));
        let first = self.segs.first().is_some_and(|s| std::ptr::eq(s, seg));
        let last = self.segs.last().is_some_and(|s| std::ptr::eq(s, seg));
        // 劇本的第一段開頭、最後一段結尾不淡出（接得上前後的劇本）
        let env = if (first && into < 0.2 && seg.anim != Anim::Still) || (last && seg.dur - into < 0.2 && into >= 0.2) {
            1.0
        } else {
            env
        };
        // 走路的步伐跟著走了多遠；原地轉身跟著時間
        let dist = if seg.anim == Anim::Turn {
            u * steps
        } else if seg.arc != 0.0 {
            u * PI * seg.arc.abs() // 迴轉走過的路
        } else {
            (x - seg.x0).abs()
        };
        let core = Pose::mix(&base, &special(base, seg.anim, tg, dist), env);
        // 轉身時頭和肩膀先轉、屁股後跟：中間最彎，頭尾回到 0
        let twist = if turning { th_head - th_hip } else { 0.0 };
        St {
            pose: common(core, tg),
            core,
            x,
            f,
            lift,
            twist,
            anim: seg.anim,
            node: seg.node,
            cap: seg.cap,
        }
    }
}

/// 這一段特有的小動作：走路的腳步、吃飯點頭、洗臉…
pub(crate) fn special(mut p: Pose, a: Anim, tg: f64, dist: f64) -> Pose {
    match a {
        Anim::Sleep => {
            let k = (tg * 1.4).sin();
            p.hip_r += 0.15 * k;
            p.chest_r += 0.12 * k;
            p.eye = 1.0;
        }
        Anim::Walk | Anim::Run => {
            // 真貓的步法：
            // 走路是「側對步」：左後 → 左前 → 右後 → 右前，每隻腳差 1/4 個週期；
            //   每隻腳約 62% 的時間踩在地上（踩著的時候腳相對地面不動，所以相對身體往後滑），
            //   其餘時間抬起來往前擺。
            // 跑是「旋轉襲步」：兩隻後腳幾乎一起蹬、兩隻前腳幾乎一起落，中間有騰空；
            //   脊椎一縮一伸，胸口和屁股前後拉開、合起。
            let run = a == Anim::Run;
            let stride = if run { 11.0 } else { 7.5 }; // 每個週期身體前進的距離
            let duty = if run { 0.38 } else { 0.62 };
            let lift_h = if run { 1.6 } else { 1.25 };
            let cyc = dist / stride;
            // 每隻腳的相位差：近側後、近側前、遠側後、遠側前
            let (o_bn, o_fn, o_bf, o_ff) = if run { (0.0, 0.55, 0.1, 0.45) } else { (0.0, 0.25, 0.5, 0.75) };
            let foot = |o: f64, front: bool| -> V {
                let ph = (cyc + o).rem_euclid(1.0);
                let reach = stride * duty / 2.0; // 踩地期間腳相對身體走過的一半
                if ph < duty {
                    // 踩在地上：從前面滑到後面
                    v(reach * (1.0 - 2.0 * ph / duty), 0.0)
                } else {
                    // 抬腳往前擺：先往上提（前腳腕往後收），再往前伸、落地
                    let s = (ph - duty) / (1.0 - duty);
                    let e = smooth(s);
                    let tuck = if front { -0.5 * (PI * s).sin() } else { 0.0 };
                    v(
                        -reach + 2.0 * reach * e + tuck,
                        lift_h * (PI * s).sin().powf(0.8) * if front { 1.0 } else { 0.85 },
                    )
                }
            };
            p.bnp = p.bnp + foot(o_bn, false);
            p.fnp = p.fnp + foot(o_fn, true);
            p.bfp = p.bfp + foot(o_bf, false);
            p.ffp = p.ffp + foot(o_ff, true);
            let w = cyc * 2.0 * PI;
            if run {
                // 脊椎縮伸、身體上下
                let g = w.cos();
                p.chest.x += 1.0 * g;
                p.hip.x -= 0.7 * g;
                p.head.x += 0.8 * g;
                p.hip.y += 0.5 * (w + 0.6).sin() - 0.7;
                p.chest.y += 0.5 * (w - 0.9).sin() - 0.5;
                p.head.y += 0.25 * (w - 0.9).sin() - 0.8;
                p.tilt -= 0.1;
                p.tail_t = p.tail_t + v(-2.5 + 0.6 * w.sin(), -2.5);
            } else {
                // 走路：肩膀和屁股各在自己那對腳踩穩時最高，頭幾乎不動
                p.hip.y += 0.16 * (2.0 * w).cos();
                p.chest.y += 0.16 * (2.0 * w + PI / 2.0).cos();
                // 走路時頭放低、往前，背比較平
                p.head.y += 0.06 * (2.0 * w + PI / 2.0).cos() - 1.1;
                p.head.x -= 0.2;
                p.tilt -= 0.12;
                p.chest.y += 0.12 * (2.0 * w + PI / 2.0).cos() - 0.2;
                p.tail_t.x += 0.7 * (w * 0.5).sin();
                p.tail_c.y += 0.3 * w.sin();
            }
        }
        Anim::Turn => {
            // dist 是走到第幾步（可以有小數）；每一步抬一組對角的腳
            let i = dist.floor();
            let fr = dist - i;
            let up = (PI * fr).sin();
            let even = (i as i64) % 2 == 0;
            let sitting = p.hip.y < 5.5;
            let lift = v(0.4 * up, 1.8 * up);
            if even {
                p.fnp = p.fnp + lift;
                if !sitting {
                    p.bfp = p.bfp + lift;
                }
            } else {
                p.ffp = p.ffp + lift;
                if !sitting {
                    p.bnp = p.bnp + lift;
                }
            }
            if sitting {
                p.hip.y += 0.25 * up;
                p.chest.y += 0.2 * up;
            } else {
                p.hip.y += 0.3 * up;
                p.chest.y += 0.35 * up;
                p.head.y += 0.2 * up;
            }
            p.tail_t.x += 0.8 * (dist * PI * 0.5).sin();
        }
        Anim::Eat => {
            let k = (tg * 7.0).sin();
            p.head.y += 0.45 * k;
            p.mouth = if k > 0.0 { 0.5 } else { 0.0 };
            p.eye = 1.0;
        }
        Anim::Groom => {
            let k = (tg * 6.0).sin();
            p.fnp = p.fnp + v(0.5 * k, 0.6 * k);
            p.tilt += 0.1 * k;
            p.mouth = if k > 0.2 { 0.5 } else { 0.0 };
            p.eye = 1.0;
        }
        Anim::Happy => {
            let k = (tg * 3.5).sin();
            p.head.x += 0.35 * k;
            p.tilt += 0.08 * k;
            p.eye = 2.0;
            p.tail_t.x += 0.8 * (tg * 5.0).sin();
        }
        Anim::Meow => {
            p.mouth = 0.6 + 0.4 * (tg * 5.0).sin().abs();
        }
        Anim::Lick => {
            p.mouth = if (tg * 6.0) as i64 % 2 == 0 { 0.5 } else { 0.0 };
            p.eye = 2.0;
        }
        Anim::Wiggle => {
            p.hip.x += 0.45 * (tg * 14.0).sin();
            p.tail_t.y += 0.8 * (tg * 9.0).sin();
        }
        Anim::Idle => {
            let look = (tg * 0.45).sin() * (tg * 0.17 + 1.0).sin();
            p.tilt += 0.18 * look;
            p.head.x += 0.25 * look;
        }
        Anim::Still => {}
    }
    p
}

/// 一直都在的小動作：呼吸、尾巴慢慢擺、眨眼
fn common(mut p: Pose, tg: f64) -> Pose {
    p.chest_r += 0.08 * (tg * 2.2).sin();
    p.tail_t.x += 0.9 * (tg * 1.1).sin();
    p.tail_c.x += 0.4 * (tg * 1.1 - 0.7).sin();
    // 眨眼：大約每 6 秒一次
    if p.eye < 0.5 && ((tg * 4.0) as u64).is_multiple_of(23) {
        p.eye = 1.0;
    }
    p
}

// ───────────────────────── 劇本內容 ─────────────────────────

fn home(w: f64) -> f64 {
    (w / 2.0).floor()
}
fn bounds(w: f64) -> (f64, f64) {
    let (lo, hi) = (17.0, w - 17.0);
    if hi < lo {
        (home(w), home(w))
    } else {
        (lo, hi)
    }
}

/// 平常的閒晃。每段 SLOT 秒，從坐在中間開始，也在那裡結束。
/// 閒晃不會睡著：睡覺只在 Claude 跑很久、或在等 shell 指令跑完的時候（外掛和 neko shell 送 doze / wake）。
fn idle(w: f64, anchor: f64, t: f64) -> (Sc, f64) {
    let k = ((t - anchor) / SLOT).floor().max(0.0);
    let s0 = anchor + k * SLOT;
    let home = home(w);
    let (lo, hi) = bounds(w);
    let mut sc = Sc::new(key(P::Sit), P::Sit, home, 1.0);
    sc.lim = (lo, hi);
    let seed = hash(s0.to_bits() ^ 0x5eed);
    {
        match [0, 1, 2, 3, 4, 1, 5, 2, 1, 3, 5, 0][(seed % 12) as usize] {
            5 => {
                sc.hold(1.0 + ((seed >> 6) % 3) as f64, Anim::Idle);
                sc.say("轉過來看你");
                sc.face_you();
                sc.hold(4.0 + ((seed >> 9) % 4) as f64, Anim::Idle);
                sc.say("坐著發呆");
                sc.face(1.0);
            }
            1 => {
                let d = 14.0 + ((seed >> 8) % 24) as f64;
                let dir = if (seed >> 4) & 1 == 0 { 1.0 } else { -1.0 };
                let mut x = home + dir * d;
                if x < lo || x > hi {
                    x = home - dir * d;
                }
                let x = x.clamp(lo, hi);
                sc.say("在散步");
                sc.walk(x, false);
                sc.say("坐下來看看");
                sc.go(P::Sit);
                sc.hold(2.5, Anim::Idle);
                sc.say("走回來");
                sc.walk(home, false);
                sc.face(1.0);
                sc.say("坐著發呆");
                sc.go(P::Sit);
            }
            2 => {
                sc.hold(1.5, Anim::Idle);
                sc.say("在洗臉");
                sc.go(P::Groom);
                sc.hold(5.0, Anim::Groom);
                sc.say("洗好臉了");
                sc.go(P::Sit);
            }
            3 => {
                sc.hold(1.5, Anim::Idle);
                sc.say("打了個呵欠");
                sc.go(P::Yawn);
                sc.hold(1.2, Anim::Still);
                sc.go(P::Sit);
                sc.say("伸懶腰");
                sc.go(P::Stretch);
                sc.hold(1.6, Anim::Still);
                sc.go(P::Stand);
                sc.say("坐著發呆");
                sc.go(P::Sit);
            }
            4 => {
                sc.say("趴下來休息");
                sc.go(P::Loaf);
                sc.hold(9.0, Anim::Idle);
                sc.go(P::Sit);
                sc.say("坐著發呆");
            }
            _ => {}
        }
    }
    if sc.node == P::Sit {
        sc.say("坐著發呆");
    }
    sc.fill(SLOT);
    (sc, s0)
}

struct From {
    pose: Pose,
    node: P,
    x: f64,
    f: f64,
}

/// 按下動作後的劇本，從記下來的姿勢接過去
fn action(act: &str, from: Option<&From>, w: f64) -> Option<Sc> {
    let home = home(w);
    let (lo, hi) = bounds(w);
    let (pose, node, x, f) = match from {
        Some(fr) => (fr.pose, fr.node, fr.x, fr.f),
        None => (key(P::Sit), P::Sit, home, 1.0),
    };
    let fs = if f < 0.0 { -1.0 } else { 1.0 };
    let mut sc = Sc::new(pose, node, x, f);
    sc.lim = (lo, hi);
    sc.say("轉過來看你");
    sc.push(0.4, key(node), node, x, fs, Anim::Still, 0.0, false);
    match act {
        "eat" => {
            let bx = (home + 20.0).min(hi + 11.0).min(w - 5.0);
            let dir = if bx >= x { 1.0 } else { -1.0 };
            let tx = (bx - dir * 11.0).clamp(lo, hi);
            sc.bowl = Some(bx);
            sc.say("聞到飯香");
            sc.walk(tx, false);
            sc.face(dir);
            sc.say("正在吃飯");
            sc.go(P::Eat);
            sc.mark("eat0");
            sc.hold(6.5, Anim::Eat);
            sc.mark("eat1");
            sc.say("吃飽了");
            sc.go(P::Stand);
            sc.say("在舔嘴巴");
            sc.go(P::Sit);
            sc.hold(2.4, Anim::Lick);
        }
        "pet" => {
            sc.say("被摸得呼嚕呼嚕");
            sc.go(P::Sit);
            sc.face_you();
            sc.go(P::Happy);
            sc.hold(3.5, Anim::Happy);
            sc.go(P::Sit);
            sc.hold(0.6, Anim::Idle);
            sc.face(1.0);
        }
        "play" => {
            let dir = if x < w / 2.0 { 1.0 } else { -1.0 };
            let bx = (if dir > 0.0 { hi + 6.0 } else { lo - 6.0 }).clamp(4.0, w - 4.0);
            sc.ball = Some((x + dir * 4.0, bx));
            sc.say("看到球了！");
            sc.go(P::Stand);
            sc.face(dir);
            sc.go(P::Crouch);
            sc.hold(0.7, Anim::Wiggle);
            sc.say("在追球");
            sc.go(P::Stand);
            sc.walk(bx - dir * 13.0, true);
            sc.go(P::Crouch);
            sc.hold(0.35, Anim::Wiggle);
            sc.say("撲！");
            sc.go(P::Air);
            let (px, ff) = (sc.x + dir * 6.0, sc.f);
            sc.push(0.45, key(P::Air), P::Air, px, ff, Anim::Still, 3.0, true);
            sc.go(P::Crouch);
            sc.go(P::Stand);
            sc.say("抓到球了");
            sc.go(P::Sit);
            sc.go(P::Happy);
            sc.hold(1.8, Anim::Happy);
            sc.go(P::Sit);
        }
        // 你送出訊息：抬頭、歪頭聽
        "look" => {
            sc.say("抬頭聽你說");
            sc.go(P::Sit);
            sc.face_you();
            let mut tilt = key(P::Sit);
            tilt.tilt += 0.35;
            tilt.head = tilt.head + v(0.3, 0.4);
            let (x0, f0) = (sc.x, sc.f);
            sc.push(0.35, tilt, P::Sit, x0, f0, Anim::Still, 0.0, false);
            sc.push(1.4, tilt, P::Sit, x0, f0, Anim::Still, 0.0, false);
            sc.push(0.4, key(P::Sit), P::Sit, x0, f0, Anim::Still, 0.0, false);
            sc.hold(1.2, Anim::Idle);
            sc.face(1.0);
        }
        // Claude 在改檔案：舉手拍兩下
        "paw" => {
            sc.say("邊看邊洗臉");
            sc.go(P::Sit);
            sc.go(P::Groom);
            sc.hold(1.6, Anim::Groom);
            sc.go(P::Sit);
            sc.hold(0.4, Anim::Idle);
        }
        "call" => {
            sc.say("回了你一聲");
            sc.go(P::Sit);
            sc.face_you();
            sc.go(P::Meow);
            sc.hold(1.6, Anim::Meow);
            sc.go(P::Sit);
            sc.hold(1.0, Anim::Idle);
        }
        "jump" => {
            let mut dir = fs;
            if x + dir * 10.0 > hi || x + dir * 10.0 < lo {
                dir = -dir;
            }
            sc.say("準備跳");
            sc.go(P::Stand);
            sc.face(dir);
            sc.go(P::Crouch);
            sc.hold(0.5, Anim::Wiggle);
            sc.say("跳了起來");
            sc.go(P::Air);
            // 跳的距離不超出活動範圍（視窗很窄時就跳近一點）
            let (jx, ff) = ((sc.x + dir * 10.0).clamp(lo.min(sc.x), hi.max(sc.x)), sc.f);
            sc.push(0.7, key(P::Air), P::Air, jx, ff, Anim::Still, 3.1, true);
            sc.go(P::Crouch);
            sc.go(P::Stand);
            sc.say("落地");
            sc.go(P::Sit);
            sc.hold(0.8, Anim::Idle);
        }
        "nap" => {
            sc.say("被你哄睡了");
            sc.go(P::Sleep);
            sc.hold(20.0, Anim::Sleep);
            sc.say("醒來了");
            sc.go(P::Loaf);
            sc.say("伸懶腰");
            sc.go(P::Stretch);
            sc.hold(1.4, Anim::Still);
            sc.go(P::Stand);
            sc.go(P::Sit);
        }
        // 等 Claude 等到睡著：一直睡，直到 "wake"
        "doze" => {
            sc.say("等到睡著了");
            sc.go(P::Sleep);
            sc.hold(4.0 * 3600.0, Anim::Sleep);
        }
        // Claude 做完了：醒來、伸懶腰、叫一聲
        "wake" => {
            sc.say("醒來了");
            sc.go(P::Loaf);
            sc.say("伸懶腰");
            sc.go(P::Stretch);
            sc.hold(1.4, Anim::Still);
            sc.go(P::Stand);
            sc.go(P::Sit);
            sc.say("跟你說好了");
            sc.face_you();
            sc.go(P::Meow);
            sc.hold(1.6, Anim::Meow);
            sc.go(P::Sit);
        }
        _ => return None,
    }
    sc.say("走回原位");
    sc.walk(home, false);
    sc.face(1.0);
    sc.say("坐著發呆");
    sc.go(P::Sit);
    sc.hold(0.5, Anim::Idle);
    Some(sc)
}

pub(crate) struct Pane {
    act: String,
    t: f64,
    w: f64,
    from: Option<From>,
}

pub(crate) fn read_pane(path: &Path) -> Option<Pane> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let from = v.get("from").and_then(|fr| {
        let a: Vec<f64> = fr.get("p")?.as_array()?.iter().filter_map(|x| x.as_f64()).collect();
        Some(From {
            pose: Pose::from(&a)?,
            node: *ALL.get(fr.get("n")?.as_u64()? as usize)?,
            x: fr.get("x")?.as_f64()?,
            f: fr.get("f")?.as_f64()?,
        })
    });
    Some(Pane {
        act: v.get("act")?.as_str()?.to_string(),
        t: v.get("t")?.as_f64()?,
        w: v.get("w").and_then(|x| x.as_f64()).unwrap_or(0.0),
        from,
    })
}

pub(crate) struct Now {
    pub(crate) st: St,
    pub(crate) bowl: Option<(f64, f64)>, // 碗的位置、剩下多少
    pub(crate) ball: Option<(f64, f64)>, // 球的位置、離地高度
}

pub(crate) fn state(pane: &Option<Pane>, t: f64, w: f64) -> Now {
    let mut anchor = 0.0;
    if let Some(pn) = pane {
        let aw = if pn.w > 0.0 { pn.w } else { w };
        if let Some(sc) = action(&pn.act, pn.from.as_ref(), aw) {
            let te = pn.t + sc.t;
            if t < te && t >= pn.t {
                let tr = t - pn.t;
                let mut st = sc.eval(tr, t);
                // 動作是用開始時的寬度 aw 排的；現在的寬度 w 不一樣（換了終端機、重開 Claude）就把位置換算過來，
                // 不然在窄的畫面上貓會停在舊的 x，被切掉一半
                let fit = |x: f64| -> f64 {
                    if aw == w {
                        return x;
                    }
                    let (alo, ahi) = bounds(aw);
                    let (lo, hi) = bounds(w);
                    if aw < w || ahi <= alo {
                        x - home(aw) + home(w) // 變寬：照原本離中間的距離放
                    } else {
                        lo + (x - alo) * (hi - lo) / (ahi - alo) // 變窄：等比例縮進新的範圍
                    }
                };
                st.x = fit(st.x).clamp(bounds(w).0.min(home(w)), bounds(w).1.max(home(w)));
                let bowl = sc.bowl.map(|bx| fit(bx).clamp(4.0, w - 4.0)).map(|bx| {
                    let (a, b) = (sc.at("eat0"), sc.at("eat1"));
                    let level = if tr < a {
                        1.0
                    } else if tr < b {
                        1.0 - (tr - a) / (b - a)
                    } else {
                        0.0
                    };
                    (bx, level)
                });
                let ball = sc.ball.map(|(s, e)| {
                    let u = (tr / 1.6).min(1.0);
                    let x = fit(lerp(s, e, 1.0 - (1.0 - u) * (1.0 - u))).clamp(4.0, w - 4.0);
                    let h = if u < 1.0 {
                        8.0 * (1.0 - u).powf(1.5) * (u * 2.5 * PI).cos().abs()
                    } else {
                        0.0
                    };
                    (x, h)
                });
                return Now { st, bowl, ball };
            }
            if t >= te {
                anchor = te;
            }
        }
    }
    let (sc, s0) = idle(w, anchor, t);
    Now {
        st: sc.eval(t - s0, t),
        bowl: None,
        ball: None,
    }
}

// ───────────────────────── 畫圖 ─────────────────────────

/// 一個姿勢展開成實際的形狀
struct Geo {
    hn: V, // 跗關節（近、遠）
    hf: V,
    tail: Vec<V>,
    ear_tip: V,
    muz: V,
    bridge: (V, V), // 鼻樑
}

impl Geo {
    fn new(p: Pose) -> Geo {
        let tb = p.hip + v(-p.hip_r * 0.85, 0.4);
        let tail = (0..=12)
            .map(|i| {
                let u = i as f64 / 12.0;
                tb * ((1.0 - u) * (1.0 - u)) + p.tail_c * (2.0 * u * (1.0 - u)) + p.tail_t * (u * u)
            })
            .collect();
        let k = p.head_r / 3.2;
        let hl = |q: V| p.head + (q * k).rot(p.tilt);
        Geo {
            hn: p.bnp + p.hock,
            hf: p.bfp + p.hock,
            tail,
            ear_tip: hl(v(1.0, 5.3)),
            muz: hl(v(2.5, -0.9)),
            bridge: (hl(v(2.2, 1.0)), hl(v(3.4, -0.1))),
        }
    }
}

struct Layer {
    w: usize,
    h: usize,
    ch: Vec<char>,
    head: (f64, f64),
}

impl Layer {
    fn new(w: usize, h: usize) -> Layer {
        Layer {
            w,
            h,
            ch: vec![' '; w * h],
            head: (0.0, 0.0),
        }
    }
    fn get(&self, x: i32, y: i32) -> char {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            ' '
        } else {
            self.ch[y as usize * self.w + x as usize]
        }
    }
    fn set(&mut self, x: i32, y: i32, c: char) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h && c != '.' {
            self.ch[y as usize * self.w + x as usize] = c;
        }
    }
    /// 小圖放大 k 倍貼上：x 是世界座標的欄，top 是最上面那列離地幾格（地面那列是 0）
    fn art(&mut self, k: f64, x: i32, top: i32, rows: &[&str]) {
        let yg = self.h as i32 - 1;
        for (j, r) in rows.iter().enumerate() {
            let up = top - j as i32;
            let (y0, y1) = (yg - ((up + 1) as f64 * k).floor() as i32 + 1, yg - (up as f64 * k).floor() as i32);
            for (i, c) in r.chars().enumerate() {
                let cx = x + i as i32;
                let (x0, x1) = ((cx as f64 * k).floor() as i32, ((cx + 1) as f64 * k).floor() as i32);
                for yy in y0..=y1 {
                    for xx in x0..x1 {
                        self.set(xx, yy, c);
                    }
                }
            }
        }
    }
}

fn color(c: char) -> u32 {
    match c {
        'k' => 0x3a2618, // 輪廓
        'o' => 0xf2a250, // 橘毛
        's' => 0xd8873a, // 毛的暗面
        't' => 0xb36a2a, // 最暗
        'g' => 0xe8974a, // 條紋亮面
        'V' => 0xb9a690, // 白毛最暗
        'P' => 0xd98494, // 粉紅暗面
        'l' => 0xffc47e, // 毛的亮面
        'd' => 0xcc7a2c, // 條紋、內線
        'D' => 0xc27026, // 遠側的腳和耳朵
        'K' => 0x8f5220,
        'w' => 0xfff6ea, // 白毛
        'W' => 0xe0d2bf,
        'p' => 0xf4a0b0, // 耳內、嘴
        'n' => 0xe7798c, // 鼻子
        'e' => 0x2b1d14, // 眼睛
        'a' => 0xa9c552, // 虹膜
        'm' => 0x9a5440, // 嘴巴的線
        'M' => 0x5a2324, // 張開的嘴裡
        'b' => 0x5b8dd9, // 碗
        'B' => 0x3f6fb8,
        'f' => 0x9a6232, // 飼料
        'r' => 0xe0483c, // 球
        'R' => 0xff9a8a,
        _ => NONE,
    }
}

fn b64(d: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(d.len() * 4 / 3 + 4);
    for ch in d.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

pub(crate) type Cell = (u32, u32, u32);

/// 整個畫面（貓、碗、球、文字）：w×h 像素，放大 k 倍；回傳每格 [字, 前景, 背景]
pub(crate) fn scene(w: usize, h: usize, k: f64, t: f64, nw: &Now) -> Vec<Cell> {
    let st = &nw.st;
    let ki = |n: f64| (n * k).round() as i32;
    let mut cv = Layer::new(w, h);
    let mut text: Vec<(i32, i32, String, u32)> = vec![];

    // 碗裡的飼料在貓後面，碗身在貓前面
    if let Some((bx, level)) = nw.bowl {
        let x = bx.round() as i32 - 4;
        if level > 0.66 {
            cv.art(k, x + 2, 4, &["ffff"]);
        }
        if level > 0.33 {
            cv.art(k, x + 1, 3, &["ffffff"]);
        } else if level > 0.02 {
            cv.art(k, x + 1, 3, &[".ff.f."]);
        }
    }
    let cat = draw_cat3(w, h, k, &st.pose, st.x, st.f, st.lift, st.twist);
    for (i, c) in cat.ch.iter().enumerate() {
        if *c != ' ' {
            cv.ch[i] = *c;
        }
    }
    if let Some((bx, _)) = nw.bowl {
        let x = bx.round() as i32 - 4;
        cv.art(k, x, 3, &["B......B", "BbbbbbbB", ".BbbbbB.", "..kkkk.."]);
    }
    if let Some((bx, hh)) = nw.ball {
        cv.art(k, bx.round() as i32 - 1, 3 + hh.round() as i32, &[".rr.", "rRrr", ".rr."]);
    }

    // 文字：zZ、愛心、喵、nom（以格為單位）
    let (hx, hy) = cat.head;
    let dir = if st.f < 0.0 { -1 } else { 1 };
    let hc = hx.round() as i32;
    let hr = (hy / 2.0).floor() as i32;
    if st.node == P::Sleep {
        let z = ["z", "zZ", "zZz", ""][((t * 1.3) as u64 % 4) as usize];
        text.push((hc + dir * ki(4.0), (hr - ki(2.0)).max(0), z.to_string(), 0x9aa7b8));
    }
    if st.anim == Anim::Happy {
        for n in 0..3 {
            let ph = ((t * 1.6 + n as f64 * 0.9) % 3.0) as i32;
            text.push((
                hc + dir * ki(4.0 + n as f64 * 3.0) + ph % 2,
                (hr - ph * ki(1.0)).max(0),
                "\u{2665}".to_string(),
                0xf4728b,
            ));
        }
    }
    if st.anim == Anim::Meow && (t * 3.0) as u64 % 3 != 2 {
        let s = "meow!";
        let c = if dir > 0 { hc + ki(5.0) } else { hc - ki(5.0) - s.len() as i32 };
        text.push((c, (hr - ki(1.0)).max(0), s.to_string(), 0xe6e6e6));
    }
    if st.anim == Anim::Eat && !((t * 3.0) as u64).is_multiple_of(3) {
        if let Some((bx, _)) = nw.bowl {
            text.push((
                (bx * k).round() as i32 + ki(3.0),
                ((h as f64 - 1.0 - 9.0 * k) / 2.0) as i32,
                "nom".to_string(),
                0xc9a26b,
            ));
        }
    }

    let rows = h / 2;
    let mut cells = Vec::with_capacity(w * rows);
    for r in 0..rows {
        for c in 0..w {
            let (top, bot) = (color(cv.ch[r * 2 * w + c]), color(cv.ch[(r * 2 + 1) * w + c]));
            let mut cell = match (top == NONE, bot == NONE) {
                (true, true) => (' ' as u32, NONE, NONE),
                (false, true) => (0x2580, top, NONE),
                (true, false) => (0x2584, bot, NONE),
                (false, false) => (0x2580, top, bot),
            };
            if cell.0 == ' ' as u32 {
                for (tx, ty, s, col) in &text {
                    if *ty == r as i32 && c as i32 >= *tx {
                        if let Some(ch) = s.chars().nth((c as i32 - tx) as usize) {
                            cell = (ch as u32, *col, NONE);
                        }
                    }
                }
            }
            cells.push(cell);
        }
    }
    cells
}

/// 一格畫面：第一行「欄 列」、第二行說明、第三行 Raster 用的 base64 格子
/// k 是放大倍率：1 是 11 列；字調小、倍率調大，同樣的實際高度能畫更多像素
/// 貓的營養狀態（neko-core line，去掉名字），例如「(o.o) 3.99kg 肚子17%」。
/// 問 Python 要 70 毫秒左右，所以在背景每 30 秒更新一次，畫畫面時只拿快取的；還沒問到時是空字串。
pub fn status_line(name: &str) -> String {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<(f64, String)>> = OnceLock::new();
    static ASKING: AtomicBool = AtomicBool::new(false);
    let cache = CACHE.get_or_init(|| Mutex::new((f64::NEG_INFINITY, String::new())));
    let (at, line) = cache.lock().map(|g| g.clone()).unwrap_or((0.0, String::new()));
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    // 存檔改過（剛餵過、API 改了）就馬上重問，不然每 30 秒問一次
    let saved = std::fs::metadata(crate::core::state_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64());
    if (t - at > 30.0 || saved > at) && !ASKING.swap(true, Ordering::SeqCst) {
        let name = name.to_string();
        std::thread::spawn(move || {
            let got = crate::core::once(&["line"]).unwrap_or_default();
            // 名字已經在行為說明的前面了，這裡拿掉
            let got = got.replacen(&format!(" {} ", name), " ", 1);
            if let Ok(mut g) = cache.lock() {
                *g = (t, got);
            }
            ASKING.store(false, Ordering::SeqCst);
        });
    }
    line
}

/// 餵食：Python 算營養、寫紀錄；成功的話貓演吃飯。回傳要顯示的訊息
pub fn feed(food: &str, pane_path: &Path) -> Result<String, String> {
    let out = crate::core::once(&["feed", food])?;
    let v: serde_json::Value = serde_json::from_str(&out).map_err(|_| format!("neko-core 回了看不懂的東西：{}", out))?;
    let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
    let name = v
        .get("cat")
        .and_then(|c| c.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("TCSG12")
        .to_string();
    let _ = act("eat", 60, 1.0, &name, pane_path);
    Ok(msg)
}

pub fn raster(cols: usize, k: f64, name: &str, pane_path: &Path) -> String {
    raster_busy(cols, k, name, pane_path).0
}

/// 同 raster，另外回傳貓是不是在動（走路、轉身、做動作）：不動的時候可以畫慢一點
pub fn raster_busy(cols: usize, k: f64, name: &str, pane_path: &Path) -> (String, bool) {
    let w = cols.clamp(40, 400);
    let k = fit_k(w, k);
    let rows = (HPX as f64 * k / 2.0).ceil() as usize;
    let t = now();
    let nw = state(&read_pane(pane_path), t, world_w(w, k));
    let cells = scene(w, rows * 2, k, t, &nw);
    let mut bytes: Vec<u8> = Vec::with_capacity(cells.len() * 12);
    for (a, b, c) in cells {
        for x in [a, b, c] {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
    }
    let calm = matches!(nw.st.anim, Anim::Idle | Anim::Sleep | Anim::Still) && nw.st.twist == 0.0 && nw.st.lift == 0.0;
    // 第二行：「名字 行為<Tab>營養狀態」（外掛會把行為換成 Claude 正在做的事，營養狀態照樣接在後面）
    (
        format!("{} {}\n{} {}\t{}\n{}\n", w, rows, name, nw.st.cap, status_line(name), b64(&bytes)),
        !calm,
    )
}

/// 倍率：世界寬度要在 40～160 格之間
pub(crate) fn fit_k(w: usize, k: f64) -> f64 {
    let lo = (w as f64 / 160.0).max(0.4);
    let hi = (w as f64 / 40.0).max(lo);
    if k.is_finite() {
        k.clamp(lo, hi)
    } else {
        lo
    }
}

pub(crate) fn world_w(w: usize, k: f64) -> f64 {
    (w as f64 / k).floor().clamp(40.0, 160.0)
}

/// 檢查用：把畫面用字母印出來（每個字母是一個像素）；k 是放大倍率
pub fn dump(cols: usize, k: f64, pane_path: &Path) -> String {
    let w = cols.clamp(40, 400);
    let h = (HPX as f64 * k).ceil() as usize;
    let nw = state(&read_pane(pane_path), now(), (w as f64 / k).max(40.0));
    let st = &nw.st;
    let cat = draw_cat3(w, h, k, &st.pose, st.x, st.f, st.lift, st.twist);
    let mut s = format!("{}  x={:.1} f={:.2} node={:?} anim={:?}\n", st.cap, st.x, st.f, st.node, st.anim);
    for j in 0..h {
        let row: String = cat.ch[j * w..(j + 1) * w].iter().collect();
        s.push_str(row.trim_end());
        s.push('\n');
    }
    s
}

/// 單獨在終端機裡跑：全螢幕，畫面越多列（字越小）貓就畫得越細
pub fn term(name: &str, pane_path: &Path) -> std::io::Result<()> {
    use crossterm::{cursor, execute, terminal};
    let mut out = std::io::stdout();
    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(terminal::ClearType::All))?;
    let res = term_loop(&mut out, name, pane_path);
    let _ = execute!(out, crossterm::style::ResetColor, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    res
}

pub(crate) fn sgr(fg: u32, bg: u32) -> String {
    let part = |c: u32, base: u8| {
        if c == NONE {
            format!("{}9", base)
        } else {
            format!("{}8;2;{};{};{}", base, c >> 16 & 255, c >> 8 & 255, c & 255)
        }
    };
    format!("\x1b[0;{};{}m", part(fg, 3), part(bg, 4))
}

fn term_loop(out: &mut std::io::Stdout, name: &str, pane_path: &Path) -> std::io::Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal;
    use std::io::Write;
    use unicode_width::UnicodeWidthChar;

    let mut zoom = 1.0f64;
    let mut msg = String::new();
    let mut msg_until = 0.0;
    loop {
        let (cols, rows) = terminal::size()?;
        let (w, rows) = (cols as usize, rows as usize);
        let mut s = String::from("\x1b[H");
        if w < 40 || rows < 8 {
            s.push_str("\x1b[0m\x1b[2J\x1b[H視窗太小了（至少 40×8），q 離開");
        } else {
            let h = (rows - 1) * 2;
            // 自動倍率：畫面高 30 個基本像素；字越小、列越多，倍率越高，貓越細
            let k = fit_k(w, h as f64 / 30.0 * zoom);
            let ww = world_w(w, k);
            let t = now();
            let nw = state(&read_pane(pane_path), t, ww);
            let cells = scene(w, h, k, t, &nw);
            let mut last = (0u32, 0u32);
            for r in 0..rows - 1 {
                s.push_str(&format!("\x1b[{};1H", r + 1));
                for c in 0..w {
                    let (ch, fg, bg) = cells[r * w + c];
                    if (fg, bg) != last {
                        s.push_str(&sgr(fg, bg));
                        last = (fg, bg);
                    }
                    s.push(char::from_u32(ch).unwrap_or(' '));
                }
            }
            // 最下面一行：說明
            let tip = if t < msg_until {
                msg.clone()
            } else {
                format!("{} {}  {}", name, nw.st.cap, status_line(name))
            };
            // 左邊是狀態，右邊是按鍵；放不下時先縮短按鍵說明，再截狀態
            let width_of = |s: &str| s.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>();
            let keys_long = format!("e 吃飯  p 摸摸  b 丟球  c 叫牠  j 跳  n 哄睡  +/- 大小（×{:.1}）  q 離開 ", k);
            let keys_short = "e吃 p摸 b球 c叫 j跳 n睡 ±大小 q離開 ".to_string();
            let left = format!("  {}", tip);
            let keys = if width_of(&left) + 2 + width_of(&keys_long) <= w {
                keys_long
            } else {
                keys_short
            };
            let room = w.saturating_sub(width_of(&keys) + 1);
            let mut fit = String::new();
            let mut used = 0;
            for ch in left.chars() {
                let cw = ch.width().unwrap_or(0);
                if used + cw > room {
                    break;
                }
                used += cw;
                fit.push(ch);
            }
            let gap = w.saturating_sub(used + width_of(&keys));
            s.push_str(&format!(
                "\x1b[{};1H\x1b[0;2m{}{}{}",
                rows,
                fit,
                " ".repeat(gap),
                if width_of(&keys) <= w { keys.as_str() } else { "" }
            ));
            s.push_str("\x1b[0m");
            out.write_all(s.as_bytes())?;
            out.flush()?;

            if event::poll(std::time::Duration::from_millis(33))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        let what = match key.code {
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                            KeyCode::Char('e') | KeyCode::Char('1') => "eat",
                            KeyCode::Char('p') | KeyCode::Char('2') => "pet",
                            KeyCode::Char('b') | KeyCode::Char('3') => "play",
                            KeyCode::Char('c') | KeyCode::Char('4') => "call",
                            KeyCode::Char('j') | KeyCode::Char('5') | KeyCode::Char(' ') => "jump",
                            KeyCode::Char('n') | KeyCode::Char('6') => "nap",
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                zoom = (zoom * 1.25).min(4.0);
                                ""
                            }
                            KeyCode::Char('-') | KeyCode::Char('_') => {
                                zoom = (zoom / 1.25).max(0.2);
                                ""
                            }
                            KeyCode::Char('0') => {
                                zoom = 1.0;
                                ""
                            }
                            _ => "",
                        };
                        if !what.is_empty() {
                            msg = act(what, w, k, name, pane_path);
                            msg_until = now() + 3.0;
                        }
                    }
                    Event::Resize(..) => out.write_all(b"\x1b[0m\x1b[2J")?,
                    _ => {}
                }
            }
            continue;
        }
        out.write_all(s.as_bytes())?;
        out.flush()?;
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
                    return Ok(());
                }
            }
        }
    }
}

pub fn act(what: &str, cols: usize, k: f64, name: &str, pane_path: &Path) -> String {
    let msg = match what {
        "eat" => format!("你放了一碗飯，{} 走過去吃。", name),
        "pet" => format!("你摸了摸 {}。", name),
        "play" => format!("你丟了一顆球給 {}。", name),
        "call" => format!("你叫了 {} 一聲。", name),
        "look" => format!("{} 抬頭看你。", name),
        "paw" => format!("{} 邊看邊洗臉。", name),
        "doze" => format!("{} 睡著了。", name),
        "wake" => format!("{} 醒來了。", name),
        "jump" => format!("{} 跳！", name),
        "nap" => format!("你哄 {} 睡覺。", name),
        _ => return "可以用：eat（吃飯）、pet（摸摸）、play（丟球）、call（叫牠）、jump（跳）、nap（哄睡）".to_string(),
    };
    let w = world_w(cols.clamp(40, 400), fit_k(cols.clamp(40, 400), k));
    let t = now();
    let st = state(&read_pane(pane_path), t, w).st;
    let v = json!({
        "act": what, "t": t, "w": w,
        "from": { "p": st.core.arr().to_vec(), "n": idx(st.node), "x": st.x, "f": st.f },
    });
    let _ = std::fs::write(pane_path, v.to_string());
    msg
}

/// 檢查用：在各種時間點觸發每個動作，逐幀找出姿勢突然跳動的地方
pub fn trace(cols: usize) -> String {
    let w = cols.clamp(40, 160) as f64;
    let base = 1_763_370_000.0;
    let mut out = String::new();
    let mut worst = 0.0f64;
    let acts = ["eat", "pet", "play", "call", "jump", "nap", "look", "paw", "wake", "doze"];
    for (ai, a) in acts.iter().enumerate() {
        for k in 0..12 {
            // 觸發時間：散布在閒晃的各個階段
            let trig = base + 20.0 * (ai * 12 + k) as f64 + 1.7 * k as f64;
            // 有時候先做另一個動作，在它中途觸發
            let prev = if k % 3 == 2 {
                let pt = trig - 2.3 - k as f64 * 0.4;
                let pst = state(&None, pt, w).st;
                Some(Pane {
                    act: acts[(ai + 1) % acts.len()].to_string(),
                    t: pt,
                    w,
                    from: Some(From {
                        pose: pst.core,
                        node: pst.node,
                        x: pst.x,
                        f: pst.f,
                    }),
                })
            } else {
                None
            };
            let st = state(&prev, trig, w).st;
            let pane = Some(Pane {
                act: a.to_string(),
                t: trig,
                w,
                from: Some(From {
                    pose: st.core,
                    node: st.node,
                    x: st.x,
                    f: st.f,
                }),
            });
            let mut last: Option<St> = None;
            let mut t = trig - 1.0;
            while t < trig + 70.0 {
                let cur = if t < trig { state(&prev, t, w).st } else { state(&pane, t, w).st };
                if let Some(l) = &last {
                    let (pa, pb) = (l.pose.arr(), cur.pose.arr());
                    let d = (0..24).map(|i| (pa[i] - pb[i]).abs()).fold(0.0, f64::max);
                    let dx = (l.x - cur.x).abs();
                    let df = (l.f - cur.f).abs();
                    let score = d.max(dx / 1.2).max(df * 2.0);
                    worst = worst.max(score);
                    let g = Geo::new(cur.pose);
                    // 耳尖＋平滑接合多出來的一點＋輪廓一格
                    let top = cur.lift + S * (g.ear_tip.y + 0.6) + 1.0;
                    if top > HPX as f64 - 1.0 {
                        out.push_str(&format!("切耳 {} k={} t={:+.2} 高 {:.1} {:?}\n", a, k, t - trig, top, cur.node));
                    }
                    if score > 0.5 {
                        out.push_str(&format!(
                            "{} k={} t={:+.2} 跳 {:.2}（姿勢 {:.2} x {:.2} 面向 {:.2}）{:?}→{:?} {}→{}\n",
                            a,
                            k,
                            t - trig,
                            score,
                            d,
                            dx,
                            df,
                            l.node,
                            cur.node,
                            l.cap,
                            cur.cap
                        ));
                    }
                }
                last = Some(cur);
                t += 0.005;
            }
        }
    }
    out.push_str(&format!("最大單幀變化 {:.2}\n", worst));
    out
}

// ───────────────────────── 立體的貓 ─────────────────────────
// 骨架還是姿勢裡那些點（x 向前、y 向上），再加上 z（左右）。每個部位是圓錐膠囊，
// 用距離場平滑地接在一起，從畫面往裡射線求交點，依法線打光。轉身就是繞 y 軸旋轉。

#[derive(Clone, Copy, Debug, Default)]
struct V3 {
    x: f64,
    y: f64,
    z: f64,
}
const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}
impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, k: f64) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
}
impl V3 {
    fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn norm(self) -> V3 {
        self * (1.0 / self.len().max(1e-9))
    }
}
fn at3(p: V, z: f64) -> V3 {
    v3(p.x, p.y, z)
}

/// 兩端半徑不同的膠囊（Inigo Quilez 的 round cone）
fn sd_rcone(p: V3, a: V3, b: V3, r1: f64, r2: f64) -> f64 {
    let ba = b - a;
    let l2 = ba.dot(ba);
    if l2 < 1e-9 {
        return (p - a).len() - r1.max(r2);
    }
    let rr = r1 - r2;
    let a2 = l2 - rr * rr;
    let il2 = 1.0 / l2;
    let pa = p - a;
    let y = pa.dot(ba);
    let z = y - l2;
    let xv = pa * l2 - ba * y;
    let x2 = xv.dot(xv);
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = rr.signum() * rr * rr * x2;
    if z.signum() * a2 * z2 > k {
        return (x2 + z2).sqrt() * il2 - r2;
    }
    if y.signum() * a2 * y2 < k {
        return (x2 + y2).sqrt() * il2 - r1;
    }
    ((x2 * a2 * il2).sqrt() + y * rr) * il2 - r1
}

/// 兩節的關節：從 a 到 b，兩段長 l1、l2，bend > 0 關節往前（+x 那側）彎；
/// 搆不到就直直拉長
fn ik(a: V, b: V, l1: f64, l2: f64, bend: f64) -> V {
    let d = b - a;
    let dl = d.len().max(1e-6);
    let dir = d * (1.0 / dl);
    if dl >= l1 + l2 - 1e-6 {
        return a + dir * (dl * l1 / (l1 + l2));
    }
    let along = (l1 * l1 - l2 * l2 + dl * dl) / (2.0 * dl);
    let h = (l1 * l1 - along * along).max(0.0).sqrt();
    // dir 往下時，(-dir.y, dir.x) 指向前方
    let side = v(-dir.y, dir.x);
    let side = if side.x * bend >= 0.0 { side } else { side * -1.0 };
    a + dir * along + side * h
}

fn smin(a: f64, b: f64, k: f64) -> f64 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

const HEAD_ZS: f64 = 1.12; // 頭的寬度 / 長度
const EYE: V3 = v3(0.70, 0.2, 0.62); // 眼睛的方向（頭座標，z 還沒乘寬度）

#[derive(Clone, Copy, PartialEq, Debug)]
enum Part {
    Body,
    Neck,
    Head,
    Muzzle,
    Ear(f64),       // 哪一側
    Leg(f64, bool), // 哪一側、是不是後腳
    Tail(f64),      // 沿著尾巴的位置 0～1
}

#[derive(Clone, Copy)]
struct Prim {
    a: V3,
    b: V3,
    r1: f64,
    r2: f64,
    zs: f64, // z 方向壓扁（身體比較窄）
    blend: f64,
    part: Part,
    paw: V3,
    bc: V3, // 外接球（加速用）
    br: f64,
}

struct Cat3 {
    p: Pose,
    prims: Vec<Prim>,
    hk: f64,
    hi: V3,
}

impl Cat3 {
    fn new(p: Pose) -> Cat3 {
        let g = Geo::new(p);
        let hk = p.head_r / 3.2;
        let hl = |q: V| p.head + (q * hk).rot(p.tilt);
        let mut prims = vec![];
        let mut add = |a: V3, b: V3, r1: f64, r2: f64, zs: f64, blend: f64, part: Part, paw: V3| {
            let bc = (a + b) * 0.5;
            let br = (b - a).len() * 0.5 + r1.max(r2) / zs.min(1.0);
            prims.push(Prim {
                a,
                b,
                r1,
                r2,
                zs,
                blend,
                part,
                paw,
                bc,
                br,
            })
        };
        let none = v3(0.0, -99.0, 0.0);
        add(at3(p.hip, 0.0), at3(p.chest, 0.0), p.hip_r, p.chest_r, 0.82, 0.0, Part::Body, none);
        add(at3(p.chest, 0.0), at3(p.head, 0.0), 2.1, 1.9, 0.9, 0.9, Part::Neck, none);
        add(at3(p.head, 0.0), at3(p.head, 0.0), p.head_r, p.head_r, HEAD_ZS, 0.8, Part::Head, none);
        // 臉頰的毛：正面看臉比較寬
        for side in [1.0, -1.0] {
            let c = hl(v(1.0, -0.9));
            add(
                v3(c.x, c.y, side * 1.5 * hk),
                v3(c.x - 0.6, c.y + 0.2, side * 1.7 * hk),
                1.75 * hk,
                1.5 * hk,
                1.0,
                0.9,
                Part::Head,
                none,
            );
        }
        add(at3(g.muz, 0.0), at3(g.bridge.1, 0.0), 1.45 * hk, 0.9 * hk, 0.85, 0.7, Part::Muzzle, none);
        add(at3(g.bridge.0, 0.0), at3(g.bridge.1, 0.0), 0.9 * hk, 0.7 * hk, 0.9, 0.7, Part::Muzzle, none);
        for side in [1.0, -1.0] {
            let base = hl(v(0.75, 2.4));
            let tip = hl(v(1.0, 5.3));
            add(
                v3(base.x, base.y, side * 1.55 * hk),
                v3(tip.x, tip.y, side * 2.0 * hk),
                1.15 * hk,
                0.18 * hk,
                1.0,
                0.25,
                Part::Ear(side),
                none,
            );
        }
        // 腳：近側 z>0、遠側 z<0
        let (fz, bz) = (1.15, 1.45);
        // 前腳：肩 → 肘（往後彎）→ 腕 → 腳掌；腳抬起來時腕往前、腳掌往後收
        for (side, paw) in [(1.0, p.fnp), (-1.0, p.ffp)] {
            let z = side * fz;
            let s = ((paw.y - 0.6) / 1.2).clamp(0.0, 1.0);
            let wrist = paw + v(0.55 * s, 0.95 - 0.25 * s);
            // 肩關節在胸口前上方；肩胛骨從背上斜下來接到肩關節
            let shoulder = p.chest + v(0.7, -0.5);
            let withers = p.chest + v(-0.6, p.chest_r * 0.8);
            let elbow = ik(shoulder, wrist, 3.0, 3.2, -1.0);
            let part = Part::Leg(side, false);
            let zs = side * (fz - 0.25);
            add(at3(withers, zs * 0.7), at3(shoulder, zs), 1.15, 1.25, 1.0, 1.0, part, at3(paw, z));
            add(at3(shoulder, zs), at3(elbow, z), 1.2, 0.85, 1.0, 0.7, part, at3(paw, z));
            add(at3(elbow, z), at3(wrist, z), 0.74, 0.62, 1.0, 0.3, part, at3(paw, z));
            add(at3(wrist, z), at3(paw, z), 0.7, 0.72, 1.0, 0.3, part, at3(paw, z));
        }
        // 後腳：髖 → 膝（往前彎）→ 跗關節（往後）→ 腳掌，貓後腳的 Z 字形
        for (side, paw, hock) in [(1.0, p.bnp, g.hn), (-1.0, p.bfp, g.hf)] {
            let z = side * bz;
            // 髖關節在屁股中間偏下；大腿肌肉從屁股上緣一路粗到膝蓋
            let hipj = p.hip + v(0.5, -0.4);
            let haunch = p.hip + v(0.2, p.hip_r * 0.45);
            let knee = ik(hipj, hock, 3.3, 2.9, 1.0);
            let part = Part::Leg(side, true);
            add(at3(haunch, z * 0.8), at3(knee, z), 2.05, 1.05, 1.0, 1.0, part, at3(paw, z));
            add(at3(knee, z), at3(hock, z), 0.78, 0.55, 1.0, 0.3, part, at3(paw, z));
            add(at3(hock, z), at3(paw, z), 0.55, 0.66, 1.0, 0.3, part, at3(paw, z));
        }
        // 腳掌
        for (side, paw, back) in [(1.0, p.fnp, false), (-1.0, p.ffp, false), (1.0, p.bnp, true), (-1.0, p.bfp, true)] {
            let z = side * if back { bz } else { fz };
            let c = paw + v(0.35, 0.15);
            add(
                at3(c - v(0.3, 0.0), z),
                at3(c + v(0.3, 0.0), z),
                0.85,
                0.85,
                1.0,
                0.3,
                Part::Leg(side, back),
                at3(paw, z),
            );
        }
        // 尾巴：往前捲的時候繞到近側
        let wrap = ((p.tail_t.x - p.hip.x) / 4.0).clamp(0.0, 1.0) * 2.9;
        let n = 8;
        for i in 0..n {
            let (u0, u1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            let i0 = (u0 * 12.0).round() as usize;
            let i1 = (u1 * 12.0).round() as usize;
            add(
                at3(g.tail[i0], wrap * u0 * u0),
                at3(g.tail[i1], wrap * u1 * u1),
                lerp(1.0, 0.72, u0),
                lerp(1.0, 0.72, u1),
                1.0,
                if i == 0 { 0.6 } else { 0.05 },
                Part::Tail(u0),
                none,
            );
        }
        let mut hi = v3(-1e9, -1e9, -1e9);
        for q in &prims {
            let r = q.r1.max(q.r2) + 0.6;
            for e in [q.a, q.b] {
                hi = v3(hi.x.max(e.x + r), hi.y.max(e.y + r), hi.z.max(e.z + r));
            }
        }
        Cat3 { p, prims, hk, hi }
    }

    fn prim_d(q: &Prim, p: V3) -> f64 {
        if q.zs == 1.0 {
            sd_rcone(p, q.a, q.b, q.r1, q.r2)
        } else {
            let s = |w: V3| v3(w.x, w.y, w.z / q.zs);
            sd_rcone(s(p), s(q.a), s(q.b), q.r1, q.r2) * q.zs
        }
    }

    /// 距離場：(距離, 最近的部位)
    fn sdf(&self, p: V3) -> (f64, usize) {
        let mut d = 1e9;
        let mut best = (1e9, 0);
        for (i, q) in self.prims.iter().enumerate() {
            let bd = (p - q.bc).len() - q.br;
            if bd > d + q.blend && bd > best.0 {
                continue; // 太遠，不影響結果
            }
            let di = Cat3::prim_d(q, p);
            if di < best.0 {
                best = (di, i);
            }
            d = if q.blend > 0.0 { smin(d, di, q.blend) } else { d.min(di) };
        }
        // 地面以下切掉
        (d.max(-p.y), best.1)
    }

    fn normal(&self, p: V3) -> V3 {
        let e = 0.04;
        let k = [v3(1.0, -1.0, -1.0), v3(-1.0, -1.0, 1.0), v3(-1.0, 1.0, -1.0), v3(1.0, 1.0, 1.0)];
        let mut n = v3(0.0, 0.0, 0.0);
        for d in k {
            n = n + d * self.sdf(p + d * e).0;
        }
        n.norm()
    }

    /// 表面上這一點的顏色（還沒打光）：'o' 毛、'd' 條紋、'w' 白、'p' 粉紅、'e' 眼睛…
    fn material(&self, p: V3, part: Part, k: f64) -> char {
        let pp = &self.p;
        let hk = self.hk;
        // 頭的座標（轉掉低頭抬頭，縮放到頭半徑 3.2）
        let hloc = |w: V3| {
            let q = v(w.x - pp.head.x, w.y - pp.head.y).rot(-pp.tilt) * (1.0 / hk);
            v3(q.x, q.y, w.z / hk)
        };
        match part {
            Part::Body => {
                let ab = pp.chest - pp.hip;
                let l = ab.len().max(1e-6);
                let d = ab * (1.0 / l);
                let nrm = v(-d.y, d.x);
                let q = v(p.x, p.y);
                let t = ((q - pp.hip).dot(d) / l).clamp(0.0, 1.0);
                let r = lerp(pp.hip_r, pp.chest_r, t);
                let off = q - (pp.hip + ab * t);
                let n = off.dot(nrm) / r;
                let s = (q - pp.hip).dot(d);
                if (n < -0.25 && t > 0.55) || n < -0.6 {
                    'w'
                } else if n > -0.3 && t > 0.08 && t < 0.85 && (s / 2.4).rem_euclid(1.0) < 0.38 {
                    'd'
                } else {
                    'o'
                }
            }
            Part::Neck => {
                let nd = pp.head - pp.chest;
                let nl = nd.len().max(1e-6);
                let nn = v(-nd.y / nl, nd.x / nl);
                let ax = (v(p.x, p.y) - pp.chest).dot(nd) / (nl * nl);
                let o = v(p.x, p.y) - (pp.chest + nd * ax.clamp(0.0, 1.0));
                if o.dot(nn) < -0.5 {
                    'w'
                } else {
                    'o'
                }
            }
            Part::Head | Part::Muzzle => self.face_mat(hloc(p), part, k),
            Part::Ear(side) => {
                let u = hloc(p);
                // 耳朵正面（朝前）中間是粉紅色
                let base = v3(0.75, 2.4, side * 1.55);
                let tip = v3(1.0, 5.3, side * 2.0);
                let ax = tip - base;
                let t = ((u - base).dot(ax) / ax.dot(ax)).clamp(0.0, 1.0);
                let c = base + ax * t;
                let rad = lerp(1.15, 0.18, t);
                let off = u - c;
                if off.x > 0.1 && v(off.y, off.z).len() < rad * 0.6 && t > 0.1 && t < 0.85 {
                    'p'
                } else {
                    'o'
                }
            }
            Part::Leg(_, _) => {
                let paw = self.prims.iter().find(|q| q.part == part && q.paw.y > -50.0).map(|q| q.paw);
                match paw {
                    Some(w) if (v(p.x, p.y) - v(w.x, w.y)).len() < 1.4 => 'w',
                    _ => 'o',
                }
            }
            Part::Tail(u) => {
                if u > 0.8 || (u * 7.0).fract() < 0.32 {
                    'd'
                } else {
                    'o'
                }
            }
        }
    }

    /// 臉：u 是頭座標（x 向前、y 向上、z 往近側），單位是頭半徑 3.2 的那個尺度
    fn face_mat(&self, u: V3, part: Part, k: f64) -> char {
        let pp = &self.p;
        let open = (1.0 - 2.0 * pp.eye).clamp(0.0, 1.0);
        let fine = k >= 1.9; // 小尺寸的五官改用投影的點畫
                             // 眼睛：頭表面上往前、往兩側的地方
        if fine && part == Part::Head {
            let dir = v3(u.x, u.y, u.z / HEAD_ZS).norm();
            for side in [1.0f64, -1.0] {
                let ec = v3(EYE.x, EYE.y, side * EYE.z).norm();
                // 在眼睛中心的切平面上量距離
                let up = v3(0.0, 1.0, 0.0);
                let tu = (up - ec * up.dot(ec)).norm();
                let ts = v3(ec.y * tu.z - ec.z * tu.y, ec.z * tu.x - ec.x * tu.z, ec.x * tu.y - ec.y * tu.x);
                let dd = dir - ec;
                if dir.dot(ec) < 0.8 {
                    continue;
                }
                let (dx, dy) = (dd.dot(ts), dd.dot(tu));
                let (rx, ry) = (0.2, 0.25 * open);
                if ry > 0.04 {
                    let n = (dx / rx).powi(2) + (dy / ry).powi(2);
                    if n <= 1.0 {
                        if k >= 3.2 {
                            let pupil = (dx / (0.06 + 0.04 * open)).powi(2) + (dy / (ry * 0.85)).powi(2) <= 1.0;
                            let rim = n > 0.72;
                            let shine = ((dx + 0.06 * side).powi(2) + (dy - ry * 0.45).powi(2)).sqrt() < 0.05;
                            return if shine {
                                'w'
                            } else if rim || pupil {
                                'e'
                            } else {
                                'a'
                            };
                        }
                        return 'e';
                    }
                } else {
                    let lw = 0.035 + 0.25 / (k * 3.2);
                    let x = (dx / 0.22).clamp(-1.0, 1.0);
                    let curve = if pp.eye < 1.5 { -0.02 + 0.07 * x * x } else { 0.07 - 0.11 * x * x };
                    if dx.abs() <= 0.22 && (dy - curve).abs() <= lw {
                        return 'e';
                    }
                }
            }
        }
        // 鼻頭、嘴巴（口鼻最前面）
        let mouth = pp.mouth.clamp(0.0, 1.0);
        if fine && u.x > 3.3 && u.z.abs() < 0.42 && u.y > -0.55 && u.y < 0.15 + 0.0 * u.z && u.y > -0.55 + u.z.abs() * 0.8 {
            return 'n';
        }
        if !fine {
        } else if mouth > 0.3 {
            let m = (mouth - 0.3) / 0.7;
            if u.x > 2.6 && u.y < -0.95 && u.y > -1.0 - 1.3 * m && u.z.abs() < 0.75 {
                return if u.y < -1.0 - 0.8 * m { 'p' } else { 'M' };
            }
        } else if u.x > 2.8 {
            let lw = 0.12 + 0.3 / k;
            // 人中往下，再往兩邊彎
            if u.z.abs() < lw * 0.8 && u.y < -0.6 && u.y > -1.15 {
                return 'm';
            }
            if (u.y - (-1.15 - 0.0 + 0.35 * (u.z.abs() - 0.45).powi(2) * 4.0 - 0.15)).abs() < lw && u.z.abs() < 0.9 && u.z.abs() > 0.05 {
                return 'm';
            }
        }
        if part == Part::Muzzle {
            return if u.y > 0.0 && u.x < 3.4 { 'o' } else { 'w' };
        }
        // 臉頰、下巴白，額頭條紋
        if u.x > 0.7 && u.y < -0.9 {
            'w'
        } else if u.y > 1.6 && u.x > -1.5 && ((u.z * 1.4 + 10.0) / 1.2).fract() < 0.4 && u.z.abs() < 1.8 {
            'd'
        } else {
            'o'
        }
    }
}

/// 打光後的顏色字元：tone 0 最亮、3 最暗
fn shade(c: char, tone: u8) -> char {
    match (c, tone) {
        ('o', 0) => 'l',
        ('o', 1) => 'o',
        ('o', 2) => 's',
        ('o', _) => 't',
        ('d', 0) => 'g',
        ('d', 1) => 'd',
        ('d', _) => 'D',
        ('w', 0) | ('w', 1) => 'w',
        ('w', 2) => 'W',
        ('w', _) => 'V',
        ('p', 0) | ('p', 1) => 'p',
        ('p', _) => 'P',
        (c, _) => c,
    }
}

/// 立體版的畫貓：f 是面向（1 向右、0 面對你、-1 向左）
#[allow(clippy::too_many_arguments)] // 姿勢、位置、面向、騰空、扭轉都是畫一隻貓需要的，包成結構反而難讀
fn draw_cat3(w: usize, h: usize, k: f64, p: &Pose, cx: f64, f: f64, lift: f64, twist: f64) -> Layer {
    let cat = Cat3::new(*p);
    let yg = h as f64 - 1.0;
    let s = S * k;
    let cxp = cx * k;
    let lp = lift * k;
    // 面向 → 繞 y 軸的角度：1 → 0（側面向右）、0 → 90°（正面）、-1 → 180°
    let th = (1.0 - f.clamp(-1.0, 1.0)) * PI / 2.0;
    // 身體沿著脊椎彎：屁股方向 th - twist/2、頭 th + twist/2，中間線性過渡，
    // 脊椎上每一點的位置是沿著方向積分出來的（像真的彎過去，不是繞中心轉）
    let (s_lo, s_hi) = (p.hip.x.min(p.head.x), p.hip.x.max(p.head.x));
    let span = (s_hi - s_lo).max(2.0);
    let c = twist / span;
    let ang = |s: f64| th + twist * (((s - s_lo) / span).clamp(0.0, 1.0) - 0.5);
    // 從 0 積分到 s：∫ (cos a, sin a)
    let seg_int = |s1: f64, s2: f64| -> (f64, f64) {
        // [s1, s2] 裡 a 是線性或常數
        let (a1, a2) = (ang(s1), ang(s2));
        if (a2 - a1).abs() < 1e-9 || c.abs() < 1e-9 {
            ((s2 - s1) * a1.cos(), (s2 - s1) * a1.sin())
        } else {
            let k = (a2 - a1) / (s2 - s1);
            ((a2.sin() - a1.sin()) / k, (a1.cos() - a2.cos()) / k)
        }
    };
    let spine = |s: f64| -> (f64, f64) {
        // 分段：s_lo 以前、s_lo..s_hi、s_hi 以後
        let mut pts = vec![0.0, s];
        for b in [s_lo, s_hi] {
            if (b > 0.0f64.min(s)) && (b < 0.0f64.max(s)) {
                pts.push(b);
            }
        }
        pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let (mut x, mut z) = (0.0, 0.0);
        for w in pts.windows(2) {
            let (dx, dz) = seg_int(w[0], w[1]);
            x += dx;
            z += dz;
        }
        if s < 0.0 {
            (-x, -z)
        } else {
            (x, z)
        }
    };
    let rotv = |m: V3, a: f64| {
        let (sn, cs) = a.sin_cos();
        v3(m.x * cs - m.z * sn, m.y, m.x * sn + m.z * cs)
    };
    let to_view = |m: V3| {
        if twist == 0.0 {
            return rotv(m, th);
        }
        let (px, pz) = spine(m.x);
        let a = ang(m.x);
        v3(px - m.z * a.sin(), m.y, pz + m.z * a.cos())
    };
    let dir_view = |d: V3, mx: f64| rotv(d, ang(mx));
    let to_model = |x: f64, y: f64, z: f64| {
        let (sn, cs) = th.sin_cos();
        let mut m = v3(x * cs + z * sn, y, -x * sn + z * cs);
        if twist != 0.0 {
            for _ in 0..5 {
                let (px, pz) = spine(m.x);
                let a = ang(m.x);
                let (dx, dz) = (x - px, z - pz);
                m.x += dx * a.cos() + dz * a.sin();
                m.z = -dx * a.sin() + dz * a.cos();
            }
        }
        m
    };
    // 光從左上前方來
    let light = v3(-0.45, 0.8, 0.55).norm();
    let mut ly = Layer::new(w, h);
    let mut depth = vec![f64::NEG_INFINITY; w * h];
    // 畫面上的範圍：每個部位的兩端加半徑（身體會扭，不能只轉外框）
    let (mut vx0, mut vx1, mut vz0, mut vz1) = (1e9f64, -1e9f64, 1e9f64, -1e9f64);
    for q in &cat.prims {
        let r = q.r1.max(q.r2) / q.zs.min(1.0) + 0.6;
        for e in [q.a, q.b] {
            let vv = to_view(e);
            vx0 = vx0.min(vv.x - r);
            vx1 = vx1.max(vv.x + r);
            vz0 = vz0.min(vv.z - r);
            vz1 = vz1.max(vv.z + r);
        }
    }
    let i0 = (cxp + vx0 * s).floor().max(0.0) as usize;
    let i1 = ((cxp + vx1 * s).ceil().max(0.0) as usize).min(w);
    let j0 = (yg - lp - cat.hi.y * s).floor().max(0.0) as usize;
    let samples: &[(f64, f64)] = if k < 1.9 {
        &[(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
    } else {
        &[(0.5, 0.5)]
    };
    let need = if samples.len() == 4 { 2 } else { 1 };
    let trace = |x: f64, y: f64| -> Option<(f64, usize, V3)> {
        let mut z = vz1;
        let mut steps = 0;
        while z > vz0 && steps < 64 {
            let m = to_model(x, y, z);
            let (d, idx) = cat.sdf(m);
            if d < 0.015 {
                return Some((z, idx, m));
            }
            z -= d.max(0.01) * if twist != 0.0 { 0.75 } else { 0.9 };
            steps += 1;
        }
        None
    };
    for j in j0..h {
        for i in i0..i1 {
            let mut hits: [(char, f64); 4] = [(' ', 0.0); 4];
            let mut n = 0;
            for &(sx, sy) in samples {
                let x = (i as f64 + sx - cxp) / s;
                let y = (yg - (j as f64 + sy) - lp) / s;
                if y < -0.05 {
                    continue;
                }
                if let Some((z, idx, m)) = trace(x, y) {
                    let part = cat.prims[idx].part;
                    let base = cat.material(m, part, k);
                    let nm = cat.normal(m);
                    let nv = dir_view(nm, m.x);
                    // 角落比較暗（環境光遮蔽）
                    let ao = (0..2)
                        .map(|a| {
                            let hh = 0.5 + a as f64 * 0.6;
                            (hh - cat.sdf(m + nm * hh).0).max(0.0) / hh
                        })
                        .sum::<f64>()
                        * 0.35;
                    let lum = (0.3 + 0.75 * nv.dot(light).max(0.0) + 0.15 * nv.z.max(0.0) - ao).clamp(0.0, 1.2);
                    let tone = if lum > 0.92 {
                        0
                    } else if lum > 0.55 {
                        1
                    } else if lum > 0.32 {
                        2
                    } else {
                        3
                    };
                    hits[n] = (shade(base, tone), z);
                    n += 1;
                }
            }
            if n >= need {
                let mut best = hits[0];
                let mut bc = 0;
                for a in 0..n {
                    let c = hits[..n].iter().filter(|hh| hh.0 == hits[a].0).count();
                    if c > bc {
                        bc = c;
                        best = hits[a];
                    }
                }
                ly.ch[j * w + i] = best.0;
                depth[j * w + i] = hits[..n].iter().map(|hh| hh.1).fold(f64::NEG_INFINITY, f64::max);
            }
        }
    }
    // 輪廓：外框，加上前後重疊的地方（深度跳很多）畫一條暗線
    let thk = (k * 0.55).round().max(1.0) as i32;
    let ring: Vec<(i32, i32)> = (-thk..=thk)
        .flat_map(|dx| (-thk..=thk).map(move |dy| (dx, dy)))
        .filter(|&(dx, dy)| (dx, dy) != (0, 0) && dx.abs() + dy.abs() <= thk)
        .collect();
    let dep = |x: i32, y: i32| -> f64 {
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            f64::NEG_INFINITY
        } else {
            depth[y as usize * w + x as usize]
        }
    };
    let mut out = ly.ch.clone();
    let lo_i = i0.saturating_sub(thk as usize + 1) as i32;
    let hi_i = (i1 + thk as usize + 1).min(w) as i32;
    for j in j0.saturating_sub(thk as usize + 1) as i32..h as i32 {
        for i in lo_i..hi_i {
            let d0 = dep(i, j);
            if d0 == f64::NEG_INFINITY {
                if ring.iter().any(|(dx, dy)| dep(i + dx, j + dy) > f64::NEG_INFINITY) {
                    out[j as usize * w + i as usize] = 'k';
                }
                continue;
            }
            let gap = 1.6;
            if ring.iter().any(|(dx, dy)| dep(i + dx, j + dy) - d0 > gap) {
                out[j as usize * w + i as usize] = 'K';
            }
        }
    }
    ly.ch = out;
    // 小尺寸：眼睛鼻子用投影的點，看得到才畫
    if k < 1.9 {
        let hk = cat.hk;
        let hw = |q: V3| {
            let r = v(q.x, q.y).rot(p.tilt) * hk;
            v3(p.head.x + r.x, p.head.y + r.y, q.z * hk)
        };
        let mut dot = |m: V3, c: char, tall: bool, wide: bool| {
            let vv = to_view(m);
            let x = (cxp + vv.x * s).floor() as i32;
            let y = (yg - lp - vv.y * s).floor() as i32;
            if (dep(x, y) - vv.z).abs() < 1.2 && !matches!(ly.get(x, y), ' ' | 'k') {
                ly.set(x, y, c);
                if tall {
                    ly.set(x, y - 1, c);
                }
                if wide {
                    ly.set(x - 1, y, c);
                }
            }
        };
        dot(hw(v3(3.75, -0.3, 0.0)), 'n', false, false);
        if p.mouth > 0.35 {
            dot(hw(v3(3.4, -1.3, 0.0)), 'p', false, false);
        }
        // 像素太少時，眼睛畫在頭裡面一點（不然會貼著輪廓），朝向你那一側才畫
        for side in [1.0, -1.0] {
            let local = v3(1.7, 0.6, side * 1.55);
            let nrm = dir_view(v3(EYE.x, EYE.y, side * EYE.z), p.head.x);
            if nrm.z < 0.05 {
                continue;
            }
            let vv = to_view(hw(local));
            let x = (cxp + vv.x * s).floor() as i32;
            let y = (yg - lp - vv.y * s).floor() as i32;
            if matches!(ly.get(x, y), ' ' | 'k') {
                continue;
            }
            ly.set(x, y, 'e');
            if p.eye < 0.5 {
                ly.set(x, y - 1, 'e');
            } else {
                ly.set(x + if x as f64 > cxp + to_view(hw(v3(0.0, 0.6, 0.0))).x * s { 1 } else { -1 }, y, 'e');
            }
        }
    }
    let hv = to_view(at3(p.head, 0.0));
    ly.head = (cxp + hv.x * s, yg - lp - hv.y * s);
    ly
}

/// 檢查用：某個姿勢、某個面向、某個倍率的樣子（字母）
pub fn pose_dump(name: &str, f: f64, k: f64, dist: f64, twist: f64) -> String {
    let node = ALL.iter().copied().find(|n| format!("{:?}", n).eq_ignore_ascii_case(name)).unwrap_or(P::Stand);
    let pose = match name {
        "walk" => special(key(P::Stand), Anim::Walk, 0.0, dist),
        "run" => special(key(P::Stand), Anim::Run, 0.0, dist),
        _ => key(node),
    };
    let w = (40.0 * k) as usize;
    let h = (HPX as f64 * k).ceil() as usize;
    let cat = draw_cat3(w, h, k, &pose, 20.0, f, 0.0, twist);
    let mut s = format!("{:?} f={}\n", node, f);
    for j in 0..h {
        let row: String = cat.ch[j * w..(j + 1) * w].iter().collect();
        s.push_str(row.trim_end());
        s.push('\n');
    }
    s
}
