//! neko — 住在終端機裡的貓（全螢幕版）。
//! 規則和寵物食量計算機一樣：每日需求 = 70 × 體重^0.75 × 1.2，時間是真的在走。
//!
//!   neko         進入房間（全螢幕，按 q 離開）
//!   neko line    印一行狀態（給 tmux 狀態列或提示字元用）

mod pix;

use chrono::{Local, Timelike};
use crossterm::{
    cursor,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde_json::{json, Value};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unicode_width::UnicodeWidthChar;

// ───────────────────────── 模型（和原本的規則相同） ─────────────────────────

const KCAL_PER_KG: f64 = 7000.0; // 體重每變化 1 kg 約等於 7000 kcal
const FACTOR: f64 = 1.2; // 結紮成貓
const IDEAL: f64 = 4.0;

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
fn need(w: f64) -> f64 {
    70.0 * w.powf(0.75) * FACTOR
}
fn stamp() -> String {
    Local::now().format("%m-%d %H:%M").to_string()
}
fn today() -> String {
    Local::now().format("%F").to_string()
}

#[derive(Clone, Copy, PartialEq)]
enum Mood {
    Starving,
    Hungry,
    Full,
    Sleep,
    Bored,
    Happy,
    Ok,
}

struct Cat {
    name: String,
    w: f64,
    belly: f64,
    joy: f64,
    born: f64,
    t: f64,
    fed_today: f64,
    day: String,
    log: Vec<(String, String)>,
    bowl_g: f64,
    bowl_kcal: f64,
}

impl Cat {
    fn new(name: Option<String>) -> Cat {
        let names = ["麻糬", "芝麻", "布丁", "饅頭", "年糕", "豆花", "肉鬆"];
        let n = now();
        let mut c = Cat {
            name: name.unwrap_or_else(|| names[(n as usize) % names.len()].to_string()),
            w: IDEAL,
            belly: need(IDEAL) / 3.0,
            joy: 70.0,
            born: n,
            t: n,
            fed_today: 0.0,
            day: today(),
            log: vec![],
            bowl_g: 0.0,
            bowl_kcal: 0.0,
        };
        let msg = format!("領養了 {}", c.name);
        c.note(&msg);
        c
    }

    fn path() -> PathBuf {
        if let Ok(p) = std::env::var("NEKO_STATE") {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join("neko/state.json")
    }

    fn load() -> Cat {
        let v: Option<Value> = std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok());
        let v = match v {
            Some(v) if v.get("w").and_then(|x| x.as_f64()).map_or(false, |w| w > 0.0) => v,
            _ => return Cat::new(None),
        };
        let f = |k: &str, d: f64| v.get(k).and_then(|x| x.as_f64()).filter(|x| x.is_finite()).unwrap_or(d);
        let s = |k: &str, d: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or(d).to_string();
        let log = v
            .get("log")
            .and_then(|l| l.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| Some((e.get(0)?.as_str()?.to_string(), e.get(1)?.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Cat {
            name: s("name", "貓"),
            w: f("w", IDEAL),
            belly: f("belly", 0.0).max(0.0),
            joy: f("joy", 50.0).clamp(0.0, 100.0),
            born: f("born", now()),
            t: f("t", now()),
            fed_today: f("fed_today", 0.0),
            day: s("day", &today()),
            log,
            bowl_g: f("bowl_g", 0.0).max(0.0),
            bowl_kcal: f("bowl_kcal", 0.0).max(0.0),
        }
    }

    fn save(&mut self) {
        let n = self.log.len();
        if n > 60 {
            self.log.drain(0..n - 60);
        }
        let v = json!({
            "name": self.name, "w": self.w, "belly": self.belly, "joy": self.joy, "born": self.born, "t": self.t,
            "fed_today": self.fed_today, "day": self.day, "bowl_g": self.bowl_g, "bowl_kcal": self.bowl_kcal,
            "log": self.log.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
        });
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }

    fn note(&mut self, msg: &str) {
        self.log.push((stamp(), msg.to_string()));
    }

    /// 經過 dt 秒：消耗熱量、肚子空了就掉體重、沒人陪就無聊。
    fn pass(&mut self, dt: f64) {
        let d = today();
        if d != self.day {
            self.day = d;
            self.fed_today = 0.0;
        }
        let days = dt / 86400.0;
        self.belly -= need(self.w) * days;
        if self.belly < 0.0 {
            self.w += self.belly / KCAL_PER_KG;
            self.belly = 0.0;
        }
        let floor = IDEAL * 0.72;
        if self.w < floor {
            self.w = floor;
            if days > 2.0 {
                self.note("你太久沒回來，鄰居幫忙餵了幾天");
            }
        }
        self.joy = (self.joy - 1.5 * dt / 3600.0).max(0.0);
    }

    /// 把真實時間算進去（上次存檔到現在）。
    fn tick(&mut self) {
        let n = now();
        let dt = (n - self.t).max(0.0);
        self.t = n;
        self.pass(dt);
    }

    /// 吃下 kcal；一天份以上的熱量存不住，直接變成體重。回傳多出來的 kcal。
    fn eat(&mut self, kcal: f64) -> f64 {
        self.belly += kcal;
        self.fed_today += kcal;
        let extra = self.belly - need(self.w);
        if extra > 0.0 {
            self.w += extra / KCAL_PER_KG;
            self.belly = need(self.w).min(self.belly - extra);
            extra
        } else {
            0.0
        }
    }

    fn bcs(&self) -> i32 {
        ((5.0 + (self.w / IDEAL - 1.0) / 0.1).round() as i32).clamp(1, 9)
    }

    fn mood(&self) -> Mood {
        let h = Local::now().hour();
        let n = need(self.w);
        if self.belly <= 0.0 {
            Mood::Starving
        } else if self.belly < n * 0.08 {
            Mood::Hungry
        } else if self.belly > n * 0.9 {
            Mood::Full
        } else if h >= 23 || h < 7 || (13..15).contains(&h) {
            Mood::Sleep
        } else if self.joy < 25.0 {
            Mood::Bored
        } else if self.joy > 60.0 {
            Mood::Happy
        } else {
            Mood::Ok
        }
    }

    fn face(&self) -> &'static str {
        match self.mood() {
            Mood::Starving => ">.<",
            Mood::Hungry => ";.;",
            Mood::Full | Mood::Sleep => "-.-",
            Mood::Bored => "-_-",
            Mood::Happy => "^.^",
            Mood::Ok => "o.o",
        }
    }
}

// ───────────────────────── 畫面緩衝區 ─────────────────────────

#[derive(Clone, Copy, PartialEq)]
struct Cell {
    ch: char,
    fg: Color,
    bg: Color,
    w: u8, // 0 = 全形字的後半格
}
const BLANK: Cell = Cell { ch: ' ', fg: Color::Reset, bg: Color::Reset, w: 1 };

struct Buf {
    w: usize,
    h: usize,
    c: Vec<Cell>,
}

impl Buf {
    fn new(w: usize, h: usize) -> Buf {
        Buf { w, h, c: vec![BLANK; w * h] }
    }
    fn set(&mut self, x: i32, y: i32, ch: char, fg: Color) {
        let cw = ch.width().unwrap_or(0);
        if cw == 0 || y < 0 || x < 0 || y as usize >= self.h || x as usize + cw > self.w {
            return;
        }
        let i = y as usize * self.w + x as usize;
        // 蓋到全形字的一半時，把另一半清掉，避免畫面錯位
        if self.c[i].w == 0 && x > 0 {
            self.c[i - 1].ch = ' ';
            self.c[i - 1].w = 1;
        }
        let last = i + cw - 1;
        if self.c[last].w == 2 && last + 1 < self.c.len() {
            self.c[last + 1].ch = ' ';
            self.c[last + 1].w = 1;
        }
        if cw == 1 && self.c[i].w == 2 && i + 1 < self.c.len() {
            self.c[i + 1].w = 1;
            self.c[i + 1].ch = ' ';
        }
        let bg = self.c[i].bg;
        self.c[i] = Cell { ch, fg, bg, w: cw as u8 };
        if cw == 2 {
            let bg2 = self.c[i + 1].bg;
            self.c[i + 1] = Cell { ch: ' ', fg, bg: bg2, w: 0 };
        }
    }
    /// 回傳字串佔的寬度
    fn text(&mut self, x: i32, y: i32, s: &str, fg: Color) -> i32 {
        let mut cx = x;
        for ch in s.chars() {
            self.set(cx, y, ch, fg);
            cx += ch.width().unwrap_or(0) as i32;
        }
        cx - x
    }
    /// 圖案：空白視為透明
    fn sprite(&mut self, x: i32, y: i32, lines: &[String], fg: Color) {
        for (j, l) in lines.iter().enumerate() {
            for (i, ch) in l.chars().enumerate() {
                if ch != ' ' {
                    self.set(x + i as i32, y + j as i32, ch, fg);
                }
            }
        }
    }
    fn bg(&mut self, x: i32, y: i32, w: i32, h: i32, bg: Color) {
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                self.c[yy as usize * self.w + xx as usize].bg = bg;
            }
        }
    }
    fn row(&self, y: usize) -> String {
        let mut s = String::with_capacity(self.w * 2);
        let (mut fg, mut bg) = (Color::Reset, Color::Reset);
        s.push_str(&format!("{}", ResetColor));
        for c in &self.c[y * self.w..(y + 1) * self.w] {
            if c.w == 0 {
                continue;
            }
            if c.bg != bg || c.fg != fg {
                if c.bg == Color::Reset || c.fg == Color::Reset {
                    s.push_str(&format!("{}", ResetColor));
                }
                if c.fg != Color::Reset {
                    s.push_str(&format!("{}", SetForegroundColor(c.fg)));
                }
                if c.bg != Color::Reset {
                    s.push_str(&format!("{}", SetBackgroundColor(c.bg)));
                }
                fg = c.fg;
                bg = c.bg;
            }
            s.push(c.ch);
        }
        s.push_str(&format!("{}", ResetColor));
        s
    }
}

fn text_width(s: &str) -> i32 {
    s.chars().map(|c| c.width().unwrap_or(0) as i32).sum()
}

// ───────────────────────── 貓的圖案 ─────────────────────────

fn lines(a: &[&str]) -> Vec<String> {
    let w = a.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    a.iter().map(|l| format!("{:<w$}", l, w = w)).collect()
}

fn mirror(a: &[String]) -> Vec<String> {
    a.iter()
        .map(|l| {
            l.chars()
                .rev()
                .map(|c| match c {
                    '/' => '\\',
                    '\\' => '/',
                    '(' => ')',
                    ')' => '(',
                    '<' => '>',
                    '>' => '<',
                    '`' => '\'',
                    '\'' => '`',
                    c => c,
                })
                .collect()
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Pose {
    Sit,
    Walk,
    Eat,
    Sleep,
    Pounce,
}

/// 所有圖案都朝右；bcs 決定坐姿的胖瘦
fn cat_sprite(pose: Pose, frame: u64, face: &str, bcs: i32, blink: bool) -> Vec<String> {
    let f = if blink { "-.-" } else { face };
    match pose {
        Pose::Sit => {
            if bcs >= 7 {
                lines(&["   /\\_/\\", &format!("  ( {} )", f), " (    \"    )/", "(____|_|____)"])
            } else if bcs <= 3 {
                lines(&["  /\\_/\\", &format!(" ( {} )", f), "  ) \" (/", "  (|_|)"])
            } else {
                lines(&["  /\\_/\\", &format!(" ( {} )", f), " (  \"  )/", " (_|_|_)"])
            }
        }
        Pose::Walk => {
            let legs = if frame % 2 == 0 { "   |_|    |_|" } else { "    \\_\\  /_/" };
            let tail = if frame % 4 < 2 { " \\        /\\_/\\" } else { " |        /\\_/\\" };
            lines(&[tail, &format!("  \\______( {} )", f), "  (   __   __ /", legs])
        }
        Pose::Pounce => lines(&["            /\\_/\\", &format!(" ~~._______( {} )", f), "   (  ___   __ /", "  /_/      \\_\\"]),
        Pose::Eat => {
            let m = if frame % 2 == 0 { "`-u-'" } else { "`-w-'" };
            lines(&[" \\", "  \\_________/\\_/\\", &format!("  (   __    ( {} )", f), &format!("   |_|  |_|  {}", m)])
        }
        Pose::Sleep => lines(&["   /\\_/\\_______", "  ( -.-        )_", "   `~~~~~~~~~~~~_)"]),
    }
}

// ───────────────────────── 房間 ─────────────────────────

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    s: &'static str,
    fg: Color,
    life: f64,
}

#[derive(PartialEq, Clone, Copy)]
enum Act {
    Sit,
    Walk(f64), // 走向某個 x（貓的中心）
    Eat,
    Sleep,
}

struct App {
    cat: Cat,
    x: f64, // 貓的中心位置
    dir: f64,
    act: Act,
    until: f64, // 這個動作做到什麼時候（app 內時間）
    time: f64,
    frame: u64,
    rng: Rng,
    parts: Vec<Particle>,
    ball: Option<(f64, f64, f64, f64, f64)>, // x, y, vx, vy, 出現時間
    laser: Option<(f64, f64, f64)>,           // x, y, 最後移動時間
    msg: String,
    msg_until: f64,
    input: Option<String>,
    meal_extra: f64,
    meal_kcal: f64,
    meal_g: f64,
    confirm_reset: f64,
    w: i32,
    h: i32,
}

const PANEL_H: i32 = 8;

impl App {
    fn floor(&self) -> i32 {
        self.h - PANEL_H - 1
    }
    fn bowl_x(&self) -> i32 {
        self.w - 13
    }
    fn bed_x(&self) -> f64 {
        12.0
    }
    fn say(&mut self, s: impl Into<String>) {
        self.msg = s.into();
        self.msg_until = self.time + 5.0;
    }
    fn hearts(&mut self, n: usize) {
        for _ in 0..n {
            let (dx, vx, vy) = (self.rng.range(-4.0, 4.0), self.rng.range(-2.0, 2.0), self.rng.range(-5.0, -2.5));
            self.parts.push(Particle { x: self.x + dx, y: self.floor() as f64 - 5.0, vx, vy, s: "<3", fg: Color::Magenta, life: 1.6 });
        }
    }

    fn feed(&mut self, g: f64, kcal_per_kg: f64, label: &str) {
        self.cat.bowl_g += g;
        self.cat.bowl_kcal += g * kcal_per_kg / 1000.0;
        // 飼料掉進碗裡
        for i in 0..5 {
            let dx = self.rng.range(1.0, 6.0);
            self.parts.push(Particle { x: self.bowl_x() as f64 + dx, y: self.floor() as f64 - 7.0 - i as f64, vx: 0.0, vy: 9.0, s: "o", fg: Color::Yellow, life: 0.7 + 0.1 * i as f64 });
        }
        self.say(format!("碗裡加了 {:.0} 克{}（{:.0} kcal）", g, label, g * kcal_per_kg / 1000.0));
    }

    fn key(&mut self, code: KeyCode) -> bool {
        // 改名字的輸入模式
        if let Some(buf) = self.input.as_mut() {
            match code {
                KeyCode::Enter => {
                    let n: String = buf.trim().chars().take(12).collect();
                    if !n.is_empty() {
                        let msg = format!("{} 改名叫 {}", self.cat.name, n);
                        self.cat.note(&msg);
                        self.cat.name = n;
                        self.say(msg);
                    }
                    self.input = None;
                }
                KeyCode::Esc => self.input = None,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) if !c.is_control() => buf.push(c),
                _ => {}
            }
            return true;
        }
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char('f') => self.feed(10.0, 3800.0, "乾糧"),
            KeyCode::Char('w') => self.feed(40.0, 1000.0, "濕食"),
            KeyCode::Char('t') => {
                self.cat.joy = (self.cat.joy + 10.0).min(100.0);
                self.feed(4.0, 3750.0, "零食");
            }
            KeyCode::Char('p') => {
                if self.cat.mood() == Mood::Starving {
                    self.say(format!("{} 餓到不想玩。先餵牠。", self.cat.name));
                } else {
                    let vx = if self.x > self.w as f64 / 2.0 { -1.0 } else { 1.0 } * self.rng.range(18.0, 34.0);
                    self.ball = Some((self.x, self.floor() as f64 - 4.0, vx, -11.0, self.time));
                }
            }
            KeyCode::Char('h') | KeyCode::Char(' ') => {
                if self.act == Act::Sleep {
                    self.say(format!("{} 在睡覺，呼嚕了一聲。", self.cat.name));
                } else {
                    self.cat.joy = (self.cat.joy + 3.0).min(100.0);
                    self.say(format!("你摸了摸 {}。", self.cat.name));
                }
                self.hearts(3);
            }
            KeyCode::Char('.') => {
                self.cat.pass(3600.0);
                self.cat.born -= 3600.0;
                self.say("快轉 1 小時。");
            }
            KeyCode::Char('n') => self.input = Some(String::new()),
            KeyCode::Char('R') => {
                if self.time < self.confirm_reset {
                    self.cat = Cat::new(None);
                    self.confirm_reset = 0.0;
                    self.say(format!("領養了 {}。", self.cat.name));
                } else {
                    self.confirm_reset = self.time + 3.0;
                    self.say("再按一次 R 就會重新領養一隻（目前這隻會離開）。");
                }
            }
            _ => {}
        }
        true
    }

    fn update(&mut self, dt: f64) {
        self.time += dt;
        self.frame += 1;
        self.cat.tick();
        let floor = self.floor() as f64;
        let mood = self.cat.mood();
        let speed = if self.cat.bcs() >= 7 { 13.0 } else { 20.0 };
        let (lo, hi) = (9.0, self.bowl_x() as f64 - 9.0); // 不要走到碗上面

        // 球的物理
        if let Some((mut bx, mut by, mut vx, mut vy, t0)) = self.ball {
            vy += 30.0 * dt;
            bx += vx * dt;
            by += vy * dt;
            if by > floor - 1.0 {
                by = floor - 1.0;
                vy = -vy * 0.55;
                vx *= 0.8;
                if vy.abs() < 2.0 {
                    vy = 0.0;
                }
            }
            if bx < 2.0 || bx > self.w as f64 - 3.0 {
                vx = -vx;
                bx = bx.clamp(2.0, self.w as f64 - 3.0);
            }
            self.ball = Some((bx, by, vx, vy, t0));
            if (bx - self.x).abs() < 4.0 && by > floor - 4.0 && self.time - t0 > 0.6 {
                self.ball = None;
                self.cat.joy = (self.cat.joy + 18.0).min(100.0);
                self.cat.belly = (self.cat.belly - 6.0).max(0.0);
                self.cat.note("玩了球");
                self.say(format!("{} 抓到球了！（消耗 6 kcal）", self.cat.name));
                self.hearts(5);
                self.act = Act::Sit;
                self.until = self.time + 1.5;
            } else if self.time - t0 > 12.0 {
                self.ball = None;
            }
        }
        if let Some((_, _, t)) = self.laser {
            if self.time - t > 1.5 {
                self.laser = None;
            }
        }

        // 決定要做什麼：玩 > 吃 > 睡 > 閒晃
        let chase = if mood == Mood::Starving {
            None
        } else if let Some((bx, ..)) = self.ball {
            Some(bx)
        } else {
            self.laser.map(|(lx, ..)| lx)
        };
        if let Some(tx) = chase {
            let tx = tx.clamp(lo, hi);
            if (tx - self.x).abs() > 2.0 {
                self.act = Act::Walk(tx);
            } else {
                if self.laser.is_some() && self.ball.is_none() {
                    // 追著雷射筆：慢慢變開心，也消耗一點熱量
                    self.cat.joy = (self.cat.joy + 4.0 * dt).min(100.0);
                    self.cat.belly = (self.cat.belly - 0.4 * dt).max(0.0);
                }
                self.act = Act::Sit;
                self.until = self.time + 0.5;
            }
        } else if self.cat.bowl_g > 0.0 {
            let tx = self.bowl_x() as f64 - 10.0;
            if (tx - self.x).abs() > 1.0 {
                self.act = Act::Walk(tx);
            } else {
                self.act = Act::Eat;
                self.dir = 1.0;
            }
        } else if self.act == Act::Eat {
            // 吃完了
            let (g, k, ex) = (self.meal_g, self.meal_kcal, self.meal_extra);
            let mut msg = format!("吃了 {:.0} 克（{:.0} kcal）", g, k);
            if ex > 0.0 {
                msg += &format!("，吃太撐，多的 {:.0} kcal 變成了 {:.0} 克體重", ex, ex / KCAL_PER_KG * 1000.0);
                self.cat.joy = (self.cat.joy - 5.0).max(0.0);
            } else {
                self.cat.joy = (self.cat.joy + 6.0).min(100.0);
                self.hearts(2);
            }
            self.cat.note(&msg);
            self.say(format!("{} {}", self.cat.name, msg));
            self.meal_g = 0.0;
            self.meal_kcal = 0.0;
            self.meal_extra = 0.0;
            self.act = Act::Sit;
            self.until = self.time + 3.0;
        } else if mood == Mood::Hungry || mood == Mood::Starving {
            let tx = self.bowl_x() as f64 - 9.0;
            if (tx - self.x).abs() > 1.0 {
                self.act = Act::Walk(tx);
            } else {
                self.act = Act::Sit;
                self.dir = 1.0;
            }
        } else if mood == Mood::Sleep {
            if (self.bed_x() - self.x).abs() > 1.0 {
                self.act = Act::Walk(self.bed_x());
            } else {
                self.act = Act::Sleep;
            }
        } else {
            match self.act {
                Act::Sleep | Act::Eat => {
                    self.act = Act::Sit;
                    self.until = self.time + 2.0;
                }
                Act::Sit if self.time > self.until => {
                    let far = if mood == Mood::Bored { 0.25 } else { 1.0 };
                    let tx = (self.x + self.rng.range(-1.0, 1.0) * self.w as f64 * 0.6 * far).clamp(lo, hi);
                    self.act = Act::Walk(tx);
                }
                _ => {}
            }
        }

        // 執行動作
        match self.act {
            Act::Walk(tx) => {
                let d = tx - self.x;
                self.dir = if d >= 0.0 { 1.0 } else { -1.0 };
                let step = speed * dt * if chase.is_some() { 1.5 } else { 1.0 };
                if d.abs() <= step {
                    self.x = tx;
                    self.act = Act::Sit;
                    self.until = self.time + self.rng.range(3.0, 9.0);
                } else {
                    self.x += self.dir * step;
                }
            }
            Act::Eat => {
                // 一秒吃 3 克
                let g = (3.0 * dt).min(self.cat.bowl_g);
                if g > 0.0 {
                    let k = self.cat.bowl_kcal * g / self.cat.bowl_g;
                    self.cat.bowl_g -= g;
                    self.cat.bowl_kcal -= k;
                    if self.cat.bowl_g < 0.05 {
                        // 碗底剩的一點點一起吃掉
                        let rest = self.cat.bowl_kcal;
                        self.meal_extra += self.cat.eat(rest);
                        self.meal_kcal += rest;
                        self.meal_g += self.cat.bowl_g;
                        self.cat.bowl_g = 0.0;
                        self.cat.bowl_kcal = 0.0;
                    }
                    self.meal_extra += self.cat.eat(k);
                    self.meal_kcal += k;
                    self.meal_g += g;
                }
                if self.frame % 8 == 0 {
                    let (px, vx) = (self.x + 8.0, self.rng.range(-1.0, 3.0));
                    self.parts.push(Particle { x: px, y: floor - 3.0, vx, vy: -3.0, s: "nom", fg: Color::Yellow, life: 0.8 });
                }
            }
            Act::Sleep => {
                if self.frame % 14 == 0 {
                    let s = if self.frame % 28 == 0 { "Z" } else { "z" };
                    self.parts.push(Particle { x: self.x + 9.0, y: floor - 4.0, vx: 1.2, vy: -1.6, s, fg: Color::Cyan, life: 2.2 });
                }
            }
            Act::Sit => {}
        }
        self.x = self.x.clamp(lo.min(hi), hi.max(lo));

        for p in self.parts.iter_mut() {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.life -= dt;
        }
        let bowl_top = floor - 2.0;
        self.parts.retain(|p| p.life > 0.0 && !(p.s == "o" && p.y > bowl_top));
    }

    fn draw(&self, b: &mut Buf) {
        let (w, h, floor) = (self.w, self.h, self.floor());
        let hour = Local::now().hour() as f64 + Local::now().minute() as f64 / 60.0;
        let night = !(6.0..18.0).contains(&hour);
        let dim = Color::DarkGrey;

        // 標題列
        b.bg(0, 0, w, 1, Color::DarkGreen);
        b.text(1, 0, &format!(" neko  {} 的房間 ", self.cat.name), Color::White);
        let clock = Local::now().format("%H:%M:%S ").to_string();
        b.text(w - clock.len() as i32 - 1, 0, &clock, Color::White);

        // 窗戶：依現在的真實時間畫太陽或月亮
        let (wx, wy, ww, wh) = (4, 2, 22, 6.min(floor - 8).max(3));
        if floor > 12 {
            let sky = if night { Color::AnsiValue(17) } else if hour < 7.5 || hour > 16.5 { Color::AnsiValue(173) } else { Color::AnsiValue(39) };
            b.bg(wx + 1, wy + 1, ww - 2, wh - 2, sky);
            for x in 0..ww {
                let c = if x == 0 || x == ww - 1 { '+' } else { '-' };
                b.set(wx + x, wy, c, Color::AnsiValue(180));
                b.set(wx + x, wy + wh - 1, c, Color::AnsiValue(180));
            }
            for y in 1..wh - 1 {
                b.set(wx, wy + y, '|', Color::AnsiValue(180));
                b.set(wx + ww - 1, wy + y, '|', Color::AnsiValue(180));
                b.set(wx + ww / 2, wy + y, '|', Color::AnsiValue(180));
            }
            let prog = if night { ((hour + 6.0) % 24.0) / 12.0 } else { (hour - 6.0) / 12.0 };
            let sx = wx + 2 + (prog * (ww - 5) as f64) as i32;
            let sy = wy + 1 + (((prog - 0.5).abs() * 2.0) * (wh - 3) as f64) as i32;
            if night {
                for i in 0..7 {
                    let (px, py) = (wx + 2 + (i * 7 + 3) % (ww - 4), wy + 1 + (i * 5) % (wh - 2).max(1));
                    let tw = (self.frame / 6 + i as u64 * 3) % 9 == 0;
                    b.set(px, py, if tw { '*' } else { '.' }, Color::White);
                }
                b.text(sx, sy, "C", Color::AnsiValue(229));
            } else {
                b.text(sx, sy, "O", Color::AnsiValue(226));
            }
            b.text(wx + ww + 3, wy + 1, &format!("[{}]", Local::now().format("%H:%M")), dim);
        }

        // 地板
        for x in 0..w {
            b.set(x, floor, '=', Color::AnsiValue(94));
        }
        // 睡墊
        b.text(self.bed_x() as i32 - 9, floor - 1, "(==================)", Color::AnsiValue(132));
        // 碗
        let bx = self.bowl_x();
        b.text(bx, floor - 1, "\\_____/", Color::AnsiValue(250));
        let n = ((self.cat.bowl_g / 10.0).ceil() as usize).min(5);
        if n > 0 {
            b.text(bx + 1, floor - 2, &"o".repeat(n), Color::Yellow);
            b.text(bx - 1, floor + 1 - 3 - 1, "", dim);
        }
        b.text(bx, floor - 3, &if self.cat.bowl_g > 0.0 { format!("{:.0}g", self.cat.bowl_g) } else { String::new() }, dim);

        // 貓
        let mood = self.cat.mood();
        let pose = match self.act {
            Act::Walk(_) if self.ball.is_some() || self.laser.is_some() => Pose::Pounce,
            Act::Walk(_) => Pose::Walk,
            Act::Eat => Pose::Eat,
            Act::Sleep => Pose::Sleep,
            Act::Sit => Pose::Sit,
        };
        let blink = pose == Pose::Sit && self.frame % 50 < 3;
        let mut sp = cat_sprite(pose, self.frame / 3, self.cat.face(), self.cat.bcs(), blink);
        if self.dir < 0.0 && pose != Pose::Sleep {
            sp = mirror(&sp);
        }
        let sw = sp[0].chars().count() as i32;
        let cy = floor - sp.len() as i32 - if pose == Pose::Sleep { 1 } else { 0 };
        let cx = self.x as i32 - sw / 2;
        let col = match mood {
            Mood::Starving => Color::AnsiValue(203),
            Mood::Hungry => Color::AnsiValue(215),
            _ => Color::AnsiValue(223),
        };
        b.sprite(cx, cy, &sp, col);

        // 對話框
        let bubble = match (mood, self.act) {
            (Mood::Starving, Act::Sit) => Some("喵！！！喵！！！"),
            (Mood::Hungry, Act::Sit) => Some("喵…（碗是空的）"),
            (Mood::Full, Act::Sit) if self.frame % 120 < 40 => Some("嗝"),
            (Mood::Bored, Act::Sit) if self.frame % 160 < 50 => Some("…"),
            _ => None,
        };
        if let Some(t) = bubble {
            let tw = text_width(t);
            let bxx = (self.x as i32 - tw / 2 - 2).clamp(0, (w - tw - 4).max(0));
            b.text(bxx, cy - 2, &format!("( {} )", t), Color::White);
            b.text(self.x as i32, cy - 1, "v", Color::White);
        }

        // 球、雷射、粒子
        if let Some((x, y, ..)) = self.ball {
            b.text(x as i32, y as i32, "@", Color::AnsiValue(208));
        }
        if let Some((x, y, _)) = self.laser {
            b.text(x as i32, y as i32, "*", Color::Red);
        }
        for p in &self.parts {
            b.text(p.x as i32, p.y as i32, p.s, p.fg);
        }

        // ── 狀態面板
        let py = h - PANEL_H;
        for x in 0..w {
            b.set(x, py, '-', dim);
        }
        let c = &self.cat;
        let (n, bcs) = (need(c.w), c.bcs());
        let body = ["", "過瘦", "過瘦", "偏瘦", "理想", "理想", "過重", "過重", "肥胖", "肥胖"][bcs as usize];
        let bcol = if bcs == 4 || bcs == 5 { Color::Green } else if bcs == 3 || bcs == 6 { Color::Yellow } else { Color::Red };
        let age = (now() - c.born) / 86400.0;
        let mut x = 2;
        x += b.text(x, py + 1, &c.name, Color::White);
        x += b.text(x, py + 1, &format!("  來你家第 {:.1} 天   體重 {:.2} kg   BCS ", age, c.w), Color::Reset);
        b.text(x, py + 1, &format!("{}（{}）", bcs, body), bcol);

        let bar = |b: &mut Buf, y: i32, label: &str, v: f64, mx: f64, fg: Color, tail: String| {
            let bw = (w - 44).clamp(10, 40);
            let fill = ((v / mx) * bw as f64).round().clamp(0.0, bw as f64) as i32;
            let mut x = 2 + b.text(2, y, label, Color::Reset);
            x += b.text(x, y, "[", dim);
            x += b.text(x, y, &"#".repeat(fill as usize), fg);
            x += b.text(x, y, &".".repeat((bw - fill) as usize), dim);
            x += b.text(x, y, "] ", dim);
            b.text(x, y, &tail, Color::Reset);
        };
        let hours = c.belly / n * 24.0;
        let tail = if hours >= 1.0 { format!("{:.0} kcal（約可撐 {:.0} 小時）", c.belly, hours) } else { format!("{:.0} kcal（快空了）", c.belly) };
        bar(b, py + 2, "肚子 ", c.belly, n, if hours < 3.0 { Color::Yellow } else { Color::Green }, tail);
        bar(b, py + 3, "心情 ", c.joy, 100.0, Color::Magenta, format!("{:.0}", c.joy));
        b.text(2, py + 4, &format!("每日需求 70 × {:.2}^0.75 × {} = {:.0} kcal ≈ {:.0} 克乾糧；今天已吃 {:.0} kcal", c.w, FACTOR, n, n / 3.8, c.fed_today), dim);

        // 訊息或建議
        let tip = if let Some(buf) = &self.input {
            (format!("新名字：{}_   （Enter 確定，Esc 取消）", buf), Color::Cyan)
        } else if self.time < self.msg_until {
            (self.msg.clone(), Color::Green)
        } else if mood == Mood::Starving {
            ("碗空了，體重正在往下掉 → 按 f 餵牠".to_string(), Color::Red)
        } else if mood == Mood::Hungry {
            ("快餓了 → 按 f".to_string(), Color::Yellow)
        } else if bcs >= 6 {
            ("變胖了：少餵一點、多陪牠玩；貓每週最多減 1–2%，不能斷食".to_string(), Color::Yellow)
        } else if bcs <= 3 {
            ("太瘦了：分幾次多餵一點".to_string(), Color::Yellow)
        } else if mood == Mood::Bored {
            ("牠很無聊 → 按 p 丟球，或用滑鼠當雷射筆".to_string(), Color::Yellow)
        } else {
            (c.log.last().map(|(t, m)| format!("{}  {}", t, m)).unwrap_or_default(), dim)
        };
        b.text(2, py + 5, &tip.0, tip.1);
        b.bg(0, h - 1, w, 1, Color::AnsiValue(236));
        b.text(1, h - 1, "f 乾糧10g  w 濕食40g  t 零食  p 丟球  h 摸摸  滑鼠 雷射筆  . 快轉1小時  n 改名  R 重新領養  q 離開", Color::AnsiValue(250));
    }
}

// ───────────────────────── 主程式 ─────────────────────────

fn restore() {
    let mut o = io::stdout();
    let _ = execute!(o, DisableMouseCapture, cursor::Show, LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

fn run() -> io::Result<()> {
    let mut cat = Cat::load();
    cat.tick();
    // 上次離開時碗裡剩下的，牠在你不在的時候吃掉了
    if cat.bowl_kcal > 0.0 {
        let k = cat.bowl_kcal;
        cat.eat(k);
        cat.bowl_g = 0.0;
        cat.bowl_kcal = 0.0;
    }
    let (w, h) = terminal::size()?;
    let mut app = App {
        cat,
        x: w as f64 / 2.0,
        dir: 1.0,
        act: Act::Sit,
        until: 2.0,
        time: 0.0,
        frame: 0,
        rng: Rng(now().to_bits() | 1),
        parts: vec![],
        ball: None,
        laser: None,
        msg: String::new(),
        msg_until: 0.0,
        input: None,
        meal_extra: 0.0,
        meal_kcal: 0.0,
        meal_g: 0.0,
        confirm_reset: 0.0,
        w: w as i32,
        h: h as i32,
    };

    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, cursor::Hide, EnableMouseCapture)?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |i| {
        restore();
        hook(i);
    }));

    let mut prev: Vec<String> = vec![];
    let mut last = Instant::now();
    let mut last_save = Instant::now();
    let frame_time = Duration::from_millis(66);
    'main: loop {
        // 輸入
        let deadline = last + frame_time;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if !event::poll(left)? {
                break;
            }
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                        break 'main;
                    }
                    if !app.key(k.code) {
                        break 'main;
                    }
                }
                Event::Mouse(m) => {
                    if matches!(m.kind, MouseEventKind::Moved | MouseEventKind::Drag(_) | MouseEventKind::Down(_)) && (m.row as i32) < app.floor() && m.row > 0 {
                        app.laser = Some((m.column as f64, m.row as f64, app.time));
                    }
                }
                Event::Resize(nw, nh) => {
                    app.w = nw as i32;
                    app.h = nh as i32;
                    prev.clear();
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        let dt = last.elapsed().as_secs_f64().min(0.25);
        last = Instant::now();

        if app.w < 60 || app.h < 20 {
            queue!(out, terminal::Clear(terminal::ClearType::All), cursor::MoveTo(0, 0), Print("視窗太小了，請放大到至少 60×20（按 q 離開）"))?;
            out.flush()?;
            prev.clear();
            continue;
        }
        app.update(dt);
        let mut buf = Buf::new(app.w as usize, app.h as usize);
        app.draw(&mut buf);
        if prev.len() != buf.h {
            prev = vec![String::new(); buf.h];
            queue!(out, terminal::Clear(terminal::ClearType::All))?;
        }
        for y in 0..buf.h {
            let r = buf.row(y);
            if r != prev[y] {
                queue!(out, cursor::MoveTo(0, y as u16), Print(&r))?;
                prev[y] = r;
            }
        }
        out.flush()?;

        if last_save.elapsed() > Duration::from_secs(5) {
            app.cat.save();
            last_save = Instant::now();
        }
    }
    app.cat.save();
    restore();
    println!("{} 目送你離開。（{:.2} kg，肚子 {:.0}%）", app.cat.name, app.cat.w, app.cat.belly / need(app.cat.w) * 100.0);
    Ok(())
}

// ───────────────────────── Claude Code 用的貓（畫法在 pix.rs） ─────────────────────────

fn pane_path() -> PathBuf {
    Cat::path().with_file_name("pane.json")
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.as_str() {
        "line" => {
            let mut c = Cat::load();
            c.tick();
            let face = match c.mood() {
                Mood::Sleep => "(-.-)zZ".to_string(),
                Mood::Starving => "(>.<)!".to_string(),
                _ => format!("({})", c.face()),
            };
            println!("{} {} {:.2}kg 肚子{:.0}%", face, c.name, c.w, c.belly / need(c.w) * 100.0);
            c.save();
        }
        "pose" => {
            let a: Vec<String> = std::env::args().collect();
            let g = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
            print!("{}", pix::pose_dump(a.get(2).map(|s| s.as_str()).unwrap_or("sit"), g(3, 1.0), g(4, 1.0), g(5, 0.0), g(6, 0.0)));
        }
        // 一直輸出畫面給 Claude Code 的 mod（每格後面接 0x1E），stdout 關掉就結束
        "stream" => {
            use std::io::Write;
            let a: Vec<String> = std::env::args().collect();
            let cols = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
            let k = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let fps: f64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(15.0);
            let name = Cat::load().name;
            let mut out = std::io::stdout().lock();
            let started = std::time::Instant::now();
            let mut last = String::new();
            let mut since_sent = 0.0;
            // 最多跑 12 小時，萬一沒人關也會自己結束
            while started.elapsed().as_secs() < 12 * 3600 {
                let t0 = std::time::Instant::now();
                let (f, busy) = pix::raster_busy(cols, k, &name, &pane_path());
                // 畫面沒變就不送；但至少每秒送一次，讓 mod 知道還活著
                if f != last || since_sent >= 1.0 {
                    if out.write_all(f.as_bytes()).and_then(|_| out.write_all(b"\x1e")).and_then(|_| out.flush()).is_err() {
                        break;
                    }
                    last = f;
                    since_sent = 0.0;
                }
                // 在動的時候用 fps，坐著不動（只有呼吸、眨眼、尾巴）每秒 5 格就夠
                let period = 1.0 / if busy { fps } else { fps.min(5.0) };
                let wait = (period - t0.elapsed().as_secs_f64()).max(0.005);
                std::thread::sleep(std::time::Duration::from_secs_f64(wait));
                since_sent += t0.elapsed().as_secs_f64();
            }
        }
        "raster" | "dump" | "do" | "trace" => {
            let args: Vec<String> = std::env::args().collect();
            let num = |i: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(60);
            let kf = |i: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let name = Cat::load().name;
            match arg.as_str() {
                "raster" => print!("{}", pix::raster(num(2), kf(3), &name, &pane_path())),
                "dump" => print!("{}", pix::dump(num(2), kf(3), &pane_path())),
                "trace" => print!("{}", pix::trace(num(2))),
                _ => println!("{}", pix::act(args.get(2).map(|s| s.as_str()).unwrap_or(""), num(3), kf(4), &name, &pane_path())),
            }
        }
        "" => {
            let name = Cat::load().name;
            if let Err(e) = pix::term(&name, &pane_path().with_file_name("pane-term.json")) {
                eprintln!("neko: {}", e);
                std::process::exit(1);
            }
        }
        // 把 shell 包起來，貓住在終端機的空白處；exit 離開
        "shell" => {
            let name = Cat::load().name;
            match pix::shell(&name, &pane_path()) {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    let _ = crossterm::terminal::disable_raw_mode();
                    eprintln!("neko: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "room" => {
            if let Err(e) = run() {
                restore();
                eprintln!("neko: {}", e);
                std::process::exit(1);
            }
        }
        _ => {
            println!(
                "neko                         全螢幕的貓（字越小畫得越細，按 q 離開）\n\
                 neko shell                   開一個 shell，貓住在終端機的空白處（exit 離開）\n\
                 neko room                    舊版房間（有熱量和體重）\n\
                 neko line                    印一行狀態（給 tmux 狀態列）\n\
                 \n\
                 給 Claude Code 的 mod 用：\n\
                 neko stream <寬> [倍率] [fps] 一直輸出畫面\n\
                 neko raster <寬> [倍率]       輸出一格畫面\n\
                 neko do <動作> <寬> [倍率]    eat pet play call jump nap look paw doze wake\n\
                 \n\
                 檢查用：\n\
                 neko pose <姿勢|walk|run> [面向] [倍率] [距離] [扭轉]  看某個姿勢\n\
                 neko dump <寬> [倍率]         用字母印出目前畫面（NEKO_NOW=<秒> 指定時間）\n\
                 neko trace [寬]               逐格檢查有沒有跳格、耳朵有沒有被切"
            );
        }
    }
}
