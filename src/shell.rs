//! neko shell：把你的 shell 包在 pty 裡，貓住在終端機的空白處。
//!
//! shell 的輸出原封不動送到真的終端機，同時用 vt100 記下畫面上每一格是什麼。
//! 貓只畫在「現在是空白、而且平常很少被寫到」的地方；輸出一碰到貓那塊，那塊立刻還原成原本的內容，
//! 貓換到別的空地（不急的話用走路、跳過去）。vim、less 這種全螢幕程式執行時貓先躲起來。
//! 最下面一列是貓的營養狀態；Ctrl+] 再按 1/2/3 餵食。指令跑超過 10 秒貓會睡著等。

use crate::pix::{act, feed, fit_k, hash, key, now, read_pane, scene, sgr, special, state, status_line, world_w, Anim, Now, St, HPX, NONE, P, ROWS};
use std::path::Path;

const SHELL_W: usize = 40; // 貓的活動範圍（欄）

fn vt_sgr(c: &vt100::Cell) -> String {
    let col = |c: vt100::Color, base: u8| match c {
        vt100::Color::Default => format!("{}9", base),
        vt100::Color::Idx(i) if i < 8 => format!("{}{}", base, i),
        vt100::Color::Idx(i) => format!("{}8;5;{}", base, i),
        vt100::Color::Rgb(r, g, b) => format!("{}8;2;{};{};{}", base, r, g, b),
    };
    let mut s = format!("\x1b[0;{};{}", col(c.fgcolor(), 3), col(c.bgcolor(), 4));
    for (on, code) in [(c.bold(), ";1"), (c.italic(), ";3"), (c.underline(), ";4"), (c.inverse(), ";7")] {
        if on {
            s.push_str(code);
        }
    }
    s.push('m');
    s
}

fn vt_blank(scr: &vt100::Screen, r: usize, c: usize) -> bool {
    match scr.cell(r as u16, c as u16) {
        Some(cell) => !cell.has_contents() && cell.bgcolor() == vt100::Color::Default && !cell.inverse(),
        None => false,
    }
}

/// 一格畫面上的字：顏色 + 內容（空的格子用空白）
fn vt_cell_bytes(scr: &vt100::Screen, r: usize, c: usize) -> Vec<u8> {
    let mut b = format!("\x1b[{};{}H", r + 1, c + 1).into_bytes();
    match scr.cell(r as u16, c as u16) {
        Some(cell) if cell.is_wide_continuation() && c > 0 => {
            // 寬字元的右半邊：從左半邊開始畫整個字
            return vt_cell_bytes(scr, r, c - 1);
        }
        Some(cell) => {
            b.extend_from_slice(vt_sgr(cell).as_bytes());
            let s = cell.contents();
            b.extend_from_slice(if s.is_empty() { b" " } else { s.as_bytes() });
        }
        None => {}
    }
    b
}

/// (x, y) 開始、SHELL_W 寬 h 高的這塊能不能放貓：現在整塊空白，而且離游標那一列至少一格
fn rect_free(scr: &vt100::Screen, x: usize, y: usize, h: usize) -> bool {
    let (rows, cols) = scr.size();
    if x + SHELL_W > cols as usize || y + h > rows as usize {
        return false;
    }
    let cy = scr.cursor_position().0 as usize;
    if cy + 1 >= y && cy <= y + h {
        return false;
    }
    (y..y + h).all(|r| (x..x + SHELL_W).all(|c| vt_blank(scr, r, c)))
}

/// 所有放得下貓的位置，好的排前面。分數：這幾欄平常越少有字越好，其次靠右、靠下，在游標上面（已經印完）比下面（等一下要印）好
fn spots(scr: &vt100::Screen, heat: &[f64], h: usize) -> Vec<(f64, usize, usize)> {
    let (rows, cols) = scr.size();
    let (rows, cols) = (rows as usize, cols as usize);
    let mut out = Vec::new();
    if cols < SHELL_W || rows < h + 2 {
        return out;
    }
    let cy = scr.cursor_position().0 as usize;
    for y in 0..=rows - h {
        for x in (0..=cols - SHELL_W).step_by(2) {
            if !rect_free(scr, x, y, h) {
                continue;
            }
            let used: f64 = heat[x..x + SHELL_W].iter().sum();
            let mut score = -used * 10.0 + x as f64 * 0.05 + y as f64 * 0.02;
            if y + h <= cy {
                score += 3.0;
            }
            out.push((score, x, y));
        }
    }
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// 這段輸出會不會讓畫面捲動（貓畫在真的終端機上，一捲就會跟著被捲走）
fn may_scroll(chunk: &[u8], old: &vt100::Screen, new: &vt100::Screen) -> bool {
    let rows = old.size().0 as usize;
    let lf = chunk.iter().filter(|&&b| b == b'\n' || b == 0x0b || b == 0x0c).count();
    // 換行只有游標走到最下面還要再換時才會捲（游標中途被往上移的情況少見，算進來就好）
    if lf > 0 && old.cursor_position().0 as usize + lf >= rows {
        return true;
    }
    // ESC D / ESC E / ESC M，CSI … S T L M J r
    let mut i = 0;
    while i + 1 < chunk.len() {
        if chunk[i] == 0x1b {
            match chunk[i + 1] {
                b'D' | b'E' | b'M' | b'c' => return true,
                b'8' if lf > 0 => return true, // 游標被還原到別處，換行會不會捲算不準
                b'[' => {
                    let mut j = i + 2;
                    while j < chunk.len() && (0x20..0x40).contains(&chunk[j]) {
                        j += 1;
                    }
                    if j < chunk.len() && matches!(chunk[j], b'S' | b'T' | b'L' | b'M' | b'r') {
                        return true;
                    }
                    i = j;
                }
                _ => {}
            }
        }
        i += 1;
    }
    // 在最後一列打字打到換行：會自己捲上去
    let last = old.size().0.saturating_sub(1);
    old.cursor_position().0 == last && new.rows(0, new.size().1).next() != old.rows(0, old.size().1).next()
}

/// NEKO_LOG=<檔案> 時記下貓換位置的原因（除錯用）
fn shell_log(msg: &str) {
    if let Some(p) = std::env::var_os("NEKO_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{:.2} {}", now(), msg);
        }
    }
}

/// 這塊有沒有被 shell 的字占用（只看內容，不管游標）
fn rect_blank(scr: &vt100::Screen, x: usize, y: usize, w: usize, h: usize) -> bool {
    let (rows, cols) = scr.size();
    x + w <= cols as usize && y + h <= rows as usize && (y..y + h).all(|r| (x..x + w).all(|c| vt_blank(scr, r, c)))
}

/// 不急的換位置：先走路（橫的），再蹲、跳、落地（直的）
struct Travel {
    col: f64,             // 貓在終端機上的欄（身體中心）
    row: usize,           // 貓那塊最上面一列
    goal: (f64, usize),   // 走到哪一欄、跳到哪一列
    via: f64,             // 在哪一欄起跳（那一欄上下整條都是空白）
    edge: bool,           // 沒有路可以跳：從畫面邊緣走出去，再從目標那一列的邊緣走進來
    dest: (usize, usize), // 到了之後貓那塊的左上角
    dist: f64,            // 走了多遠（給步伐用）
    f: f64,               // 面向
    hop: u32,             // 跳的進度：0 還沒跳，之後每格 +1
    prev: (f64, usize),   // 上一步在哪（前面被擋住時從這裡重新規劃）
    replans: u32,         // 重新規劃了幾次
}

/// 這段輸出有沒有清畫面（CSI J；CSI K 只清游標那一列，貓不會在那裡）：貓畫在真的終端機上，被清掉要重畫，但貓不用換位置
fn chunk_erases(chunk: &[u8]) -> bool {
    let mut i = 0;
    while i + 1 < chunk.len() {
        if chunk[i] == 0x1b && chunk[i + 1] == b'[' {
            let mut j = i + 2;
            while j < chunk.len() && (0x20..0x40).contains(&chunk[j]) {
                j += 1;
            }
            if j < chunk.len() && chunk[j] == b'J' {
                return true;
            }
            i = j;
        }
        i += 1;
    }
    false
}

/// 畫在終端機上的貓
struct Overlay {
    pos: Option<(usize, usize)>, // 貓那塊的左上角（坐著、閒晃時）
    travel: Option<Travel>,
    h: usize,
    k: f64,
    drawn: std::collections::HashMap<(usize, usize), (u32, u32, u32)>, // 現在畫在終端機上的貓的格子
    next_wander: f64,
}

impl Overlay {
    /// 把貓的格子全部還原成 shell 原本的內容
    fn erase(&mut self, scr: &vt100::Screen, buf: &mut Vec<u8>) {
        for &(r, c) in self.drawn.keys() {
            buf.extend_from_slice(&vt_cell_bytes(scr, r, c));
        }
        self.drawn.clear();
    }

    /// 急的讓位：畫面捲動了（位置全部跑掉）就不走了，重新找位置
    fn bumped(&mut self, why: &str) {
        shell_log(&format!("bumped: {}", why));
        if self.travel.is_some() {
            self.travel = None;
        }
    }

    /// 開始走去 (x, y) 那塊：從貓現在站的地方出發
    fn go(&mut self, scr: &vt100::Screen, x: usize, y: usize, t: f64, pane_path: &Path) {
        let k = self.k;
        let ww = world_w(SHELL_W, k);
        let st = state(&read_pane(pane_path), t, ww).st;
        // 從貓現在在的地方出發：走到一半（重新規劃）就從半路，不然從坐著的那塊
        let (col, py, px) = match (&self.travel, self.pos) {
            (Some(tr), _) => {
                if tr.replans >= 2 {
                    shell_log("replanned too often: jump straight there");
                    self.travel = None;
                    self.pos = Some((x, y));
                    return;
                }
                (tr.col, tr.row, tr.col as usize)
            }
            (None, Some((px, py))) => (px as f64 + st.x * k, py, px),
            _ => return,
        };
        let replans = self.travel.as_ref().map_or(0, |tr| tr.replans + 1);
        self.travel = None;
        // 到了之後，貓在新那塊裡站的位置跟現在一樣（閒晃的劇本接得上）
        let goal = x as f64 + st.x * k;
        let f = if goal >= col { 1.0 } else { -1.0 };
        // 規劃路線：沿著現在這一列走到起跳的那一欄 → 跳到目標那一列 → 再走到目的地。
        // 走過、跳過的地方都要是空白（不然會蓋到字）；找不到路就直接過去
        // 貓身體左右各佔幾欄：把走路、蹲、騰空的姿勢實際畫一次量出來（兩個方向都量）
        let half = {
            let ww = world_w(SHELL_W, k);
            let mut most = 0i64;
            for (pose, node, f) in [
                (special(key(P::Stand), Anim::Walk, t, 0.0), P::Stand, 1.0),
                (special(key(P::Stand), Anim::Walk, t, 3.0), P::Stand, -1.0),
                (key(P::Crouch), P::Crouch, 1.0),
                (key(P::Air), P::Air, 1.0),
                (key(P::Air), P::Air, -1.0),
            ] {
                let st = St {
                    pose,
                    core: pose,
                    x: ww / 2.0,
                    f,
                    lift: 0.0,
                    twist: 0.0,
                    anim: Anim::Still,
                    node,
                    cap: "",
                };
                let cells = scene(SHELL_W, self.h * 2, k, t, &Now { st, bowl: None, ball: None });
                let mid = (ww / 2.0 * k) as i64;
                for (i, c) in cells.iter().enumerate() {
                    if !(c.0 == ' ' as u32 && c.2 == NONE) {
                        most = most.max(((i % SHELL_W) as i64 - mid).abs());
                    }
                }
            }
            most + 1
        };
        let h = self.h;
        let cols = scr.size().1 as i64;
        let band = |c0: f64, c1: f64, r0: usize, r1: usize| {
            // 超出畫面的部分畫不出來，不用檢查
            let (a, b) = ((c0.min(c1).round() as i64 - half).max(0), (c0.max(c1).round() as i64 + half).min(cols - 1));
            a <= b && rect_blank(scr, a as usize, r0, (b - a + 1) as usize, r1 - r0)
        };
        let via = if y == py {
            Some(goal)
        } else {
            let (r0, r1) = (py.min(y), py.max(y) + h);
            (0..cols)
                .map(|c| c as f64)
                .filter(|&c| band(c, c, r0, r1) && band(col, c, py, py + h) && band(c, goal, y, y + h))
                .min_by(|a, b| {
                    let d = |c: f64| (c - col).abs() + (c - goal).abs();
                    d(*a).partial_cmp(&d(*b)).unwrap_or(std::cmp::Ordering::Equal)
                })
        };
        // 跳不過去：看左右哪一邊的兩列都空著，從那一邊走出去再走進來
        let mut edge = false;
        let via = via.or_else(|| {
            let right = (cols + half) as f64;
            let left = -(half as f64) - 1.0;
            let ok = |e: f64| {
                let inside = if e > 0.0 { (cols - 1) as f64 } else { 0.0 };
                band(col, inside, py, py + h) && band(inside, goal, y, y + h)
            };
            let side = [right, left].into_iter().filter(|&e| ok(e)).min_by(|a, b| {
                let d = |e: f64| (e - col).abs() + (e - goal).abs();
                d(*a).partial_cmp(&d(*b)).unwrap_or(std::cmp::Ordering::Equal)
            });
            edge = side.is_some();
            side
        });
        let Some(via) = via else {
            let c = col.round();
            shell_log(&format!(
                "no path ({},{}) -> ({},{}): jump straight there [col {:.1} goal {:.1} half {} h {} rows {:?} | up {} start {} end {}]",
                px,
                py,
                x,
                y,
                col,
                goal,
                half,
                h,
                scr.size(),
                band(c, c, py.min(y), py.max(y) + h),
                band(col, c, py, py + h),
                band(c, goal, y, y + h)
            ));
            self.pos = Some((x, y));
            return;
        };
        shell_log(&format!(
            "go ({},{}) -> ({},{}) via col {}{}",
            px,
            py,
            x,
            y,
            via,
            if edge { " (off screen)" } else { "" }
        ));
        self.travel = Some(Travel {
            col,
            row: py,
            goal: (goal, y),
            via,
            edge,
            dest: (x, y),
            dist: 0.0,
            f,
            hop: 0,
            prev: (col, py),
            replans,
        });
        self.pos = None;
    }

    /// 走路中的一格：回傳要畫的貓（Now）和那塊的左上角；走到了回傳 None
    fn step_travel(&mut self, scr: &vt100::Screen, t: f64) -> Option<(Now, usize, usize)> {
        let k = self.k;
        let h = self.h;
        let (cols, _) = (scr.size().1 as usize, ());
        let tr = self.travel.as_mut()?;
        tr.prev = (tr.col, tr.row);
        // 還沒跳到目標那一列：先走到起跳的那一欄；跳完再走到目的地
        let aim = if tr.row != tr.goal.1 { tr.via } else { tr.goal.0 };
        let walking = (aim - tr.col).abs() >= 0.5 && (tr.hop == 0 || tr.hop >= 1000);
        let (pose, node, anim, lift);
        if walking {
            // 橫的：每格走 1 欄
            let d = (aim - tr.col).clamp(-1.0, 1.0);
            tr.col += d;
            tr.dist += d.abs() / k;
            tr.f = d.signum();
            pose = special(key(P::Stand), Anim::Walk, t, tr.dist);
            node = P::Stand;
            anim = Anim::Walk;
            lift = 0.0;
        } else if tr.row != tr.goal.1 && tr.edge {
            // 已經走出畫面：直接換到目標那一列，接著從邊緣走進來
            tr.row = tr.goal.1;
            tr.hop = 1000;
            pose = special(key(P::Stand), Anim::Walk, t, tr.dist);
            node = P::Stand;
            anim = Anim::Walk;
            lift = 0.0;
        } else if tr.row != tr.goal.1 {
            // 直的：蹲兩格、騰空每格一列、落地蹲一格
            tr.hop += 1;
            if tr.hop <= 2 {
                pose = key(P::Crouch);
                node = P::Crouch;
                anim = Anim::Still;
            } else {
                tr.row = if tr.goal.1 > tr.row { tr.row + 1 } else { tr.row - 1 };
                pose = key(P::Air);
                node = P::Air;
                anim = Anim::Still;
            }
            lift = 0.0;
        } else if tr.hop > 0 && tr.hop < 1000 {
            tr.hop = 1000; // 落地（之後如果還沒到目的地，再走過去）
            pose = key(P::Crouch);
            node = P::Crouch;
            anim = Anim::Still;
            lift = 0.0;
        } else {
            return None;
        }
        // 貓那塊跟著貓走：貓在塊的中間，靠邊時塊貼邊、貓在塊裡偏一邊
        let bx = (tr.col - SHELL_W as f64 / 2.0).round().clamp(0.0, cols.saturating_sub(SHELL_W) as f64) as usize;
        let x = (tr.col - bx as f64) / k;
        let st = St {
            pose,
            core: pose,
            x,
            f: tr.f,
            lift,
            twist: 0.0,
            anim,
            node,
            cap: "",
        };
        let _ = h;
        Some((Now { st, bowl: None, ball: None }, bx, tr.row))
    }

    /// 這一格不動：已經畫的貓留著（碰到字的格子還原）
    fn frame_hold(&mut self, scr: &vt100::Screen, buf: &mut Vec<u8>) {
        let bad: Vec<_> = self.drawn.keys().filter(|&&(r, c)| !vt_blank(scr, r, c)).copied().collect();
        for (r, c) in bad {
            buf.extend_from_slice(&vt_cell_bytes(scr, r, c));
            self.drawn.remove(&(r, c));
        }
    }

    /// 畫一格：決定位置、走一步、只送有變的格子
    fn frame(&mut self, scr: &vt100::Screen, heat: &[f64], t: f64, name: &str, pane_path: &Path, buf: &mut Vec<u8>) {
        let _ = name;
        let (rows, _) = scr.size();
        if scr.alternate_screen() {
            // 全螢幕程式在用：畫面是它的，貓躲起來（不用還原，它會自己畫）
            self.drawn.clear();
            self.pos = None;
            self.travel = None;
            return;
        }
        // 貓的大小：畫面高度的三分之一以內，最多 1 倍
        let k = fit_k(SHELL_W, (rows as f64 / 3.0 / ROWS as f64).clamp(0.4, 1.0));
        let h = (HPX as f64 * k / 2.0).ceil() as usize;
        if h != self.h {
            self.pos = None;
            self.travel = None;
        }
        self.k = k;
        self.h = h;
        let near = |all: &Vec<(f64, usize, usize)>, ox: usize, oy: usize| {
            all.iter()
                .max_by(|a, b| {
                    let d = |s: &(f64, usize, usize)| s.0 - ((s.1 as f64 - ox as f64).abs() + (s.2 as f64 - oy as f64).abs()) * 0.05;
                    d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
                })
                .copied()
        };
        if let Some((x, y)) = self.pos {
            if !rect_blank(scr, x, y, SHELL_W, h) {
                // 急：那塊已經有字了（正常不會到這裡，輸出時就讓掉了）→ 直接換
                shell_log("teleport: spot has text");
                let all = spots(scr, heat, h);
                self.pos = near(&all, x, y).map(|(_, x, y)| (x, y));
                self.next_wander = t + 8.0;
            } else if !rect_free(scr, x, y, h) {
                // 不急：還空著，只是游標靠過來了 → 走過去 / 跳過去
                let all = spots(scr, heat, h);
                if let Some((_, nx, ny)) = near(&all, x, y) {
                    self.go(scr, nx, ny, t, pane_path);
                }
                self.next_wander = t + 8.0;
            }
        } else if self.travel.is_none() {
            self.pos = spots(scr, heat, h).first().map(|&(_, x, y)| (x, y));
            shell_log(&format!("place {:?}", self.pos));
            self.next_wander = t + 8.0;
        }
        // 閒晃：偶爾挑另一塊空地走過去
        if let (Some((x, y)), None) = (self.pos, &self.travel) {
            if t >= self.next_wander {
                let r = hash(t.to_bits());
                self.next_wander = t + 12.0 + (r % 25) as f64;
                let all = spots(scr, heat, h);
                let good: Vec<_> = all.iter().take(40).filter(|s| s.1.abs_diff(x) + s.2.abs_diff(y) >= 4).collect();
                if !good.is_empty() {
                    let s = good[(r >> 8) as usize % good.len()];
                    self.go(scr, s.1, s.2, t, pane_path);
                }
            }
        }
        // 要畫的貓
        let mut frame_at: Option<(Now, usize, usize)> = None;
        if self.travel.is_some() {
            match self.step_travel(scr, t) {
                Some(f) => frame_at = Some(f),
                None => {
                    let tr = self.travel.take().unwrap();
                    // 走到了；目的地這時被占了就就近再找
                    self.pos = if rect_free(scr, tr.dest.0, tr.dest.1, h) {
                        Some(tr.dest)
                    } else {
                        near(&spots(scr, heat, h), tr.dest.0, tr.dest.1).map(|(_, x, y)| (x, y))
                    };
                }
            }
        }
        if frame_at.is_none() {
            if let Some((x, y)) = self.pos {
                frame_at = Some((state(&read_pane(pane_path), t, world_w(SHELL_W, k)), x, y));
            }
        }
        let mut want = std::collections::HashMap::new();
        if let Some((nw, x, y)) = frame_at {
            let cells = scene(SHELL_W, h * 2, k, t, &nw);
            for r in 0..h {
                for c in 0..SHELL_W {
                    let cell = cells[r * SHELL_W + c];
                    if !(cell.0 == ' ' as u32 && cell.2 == NONE) {
                        want.insert((y + r, x + c), cell);
                    }
                }
            }
            // 走路途中前面被字擋住（路是出發時規劃的，之後畫面又變了）：從半路重新規劃
            if self.travel.is_some() && want.keys().any(|&(r, c)| !vt_blank(scr, r, c)) {
                let tr = self.travel.as_mut().unwrap();
                (tr.col, tr.row) = tr.prev; // 退回上一步再規劃
                let (dx, dy) = (tr.dest.0, tr.dest.1);
                let dest = if rect_free(scr, dx, dy, h) {
                    Some((dx, dy))
                } else {
                    near(&spots(scr, heat, h), dx, dy).map(|(_, x, y)| (x, y))
                };
                if let Some((nx, ny)) = dest {
                    shell_log("path blocked: replan");
                    self.go(scr, nx, ny, t, pane_path);
                    if self.travel.is_some() {
                        // 這一格先停在原地（不畫踩到字的那一步），下一格照新路線走
                        return self.frame_hold(scr, buf);
                    }
                }
            }
            if self.travel.is_some() && want.keys().any(|&(r, c)| !vt_blank(scr, r, c)) {
                let tr = self.travel.take().unwrap();
                shell_log("teleport: path blocked");
                self.pos = None;
                want.clear();
                if rect_free(scr, tr.dest.0, tr.dest.1, h) {
                    self.pos = Some(tr.dest);
                    let nw = state(&read_pane(pane_path), t, world_w(SHELL_W, k));
                    let cells = scene(SHELL_W, h * 2, k, t, &nw);
                    for r in 0..h {
                        for c in 0..SHELL_W {
                            let cell = cells[r * SHELL_W + c];
                            if !(cell.0 == ' ' as u32 && cell.2 == NONE) {
                                want.insert((tr.dest.1 + r, tr.dest.0 + c), cell);
                            }
                        }
                    }
                }
            }
        }
        // 不再是貓的格子：還原
        let gone: Vec<_> = self.drawn.keys().filter(|p| !want.contains_key(p)).copied().collect();
        for (r, c) in gone {
            buf.extend_from_slice(&vt_cell_bytes(scr, r, c));
        }
        // 有變的格子：畫上去
        let mut last = (u32::MAX, u32::MAX);
        let mut keys: Vec<_> = want.iter().filter(|(p, v)| self.drawn.get(p) != Some(v)).map(|(p, v)| (*p, *v)).collect();
        keys.sort_by_key(|(p, _)| *p);
        let mut at = (usize::MAX, usize::MAX);
        for ((r, c), (ch, fg, bg)) in keys {
            if at != (r, c) {
                buf.extend_from_slice(format!("\x1b[{};{}H", r + 1, c + 1).as_bytes());
                last = (u32::MAX, u32::MAX);
            }
            if last != (fg, bg) {
                buf.extend_from_slice(sgr(fg, bg).as_bytes());
                last = (fg, bg);
            }
            let mut tmp = [0u8; 4];
            buf.extend_from_slice(char::from_u32(ch).unwrap_or(' ').encode_utf8(&mut tmp).as_bytes());
            at = (r, c + 1);
        }
        self.drawn = want;
    }
}

fn winsize(fd: i32) -> (u16, u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (24, 80)
    }
}

/// neko shell 最下面那一列：貓的營養狀態（餵食是 Ctrl+] 再按 1/2/3，畫面上不顯示）
struct Bar {
    armed: bool, // 按了 Ctrl+]，等數字
    msg: String, // 剛餵完的訊息
    msg_until: f64,
    feeding: bool, // 正在等 Python 回覆
    drawn: String, // 上次畫的內容（一樣就不重畫）
}

const FOODS: [(&str, &str); 3] = [("dry", "乾糧"), ("wet", "濕食"), ("treat", "零食")];

impl Bar {
    /// 這一列要顯示的字（已經截到 cols 寬）
    fn text(&self, name: &str, cols: usize, t: f64) -> (String, String) {
        use unicode_width::UnicodeWidthStr;
        // 左邊：剛餵完的訊息，不然是營養狀態（把名字放回表情後面：(o.o) 芝麻 3.99kg 肚子16%）
        let left = if t < self.msg_until {
            self.msg.clone()
        } else if self.feeding {
            "餵食中…".to_string()
        } else {
            let st = status_line(name);
            match st.split_once(' ') {
                Some((face, rest)) => format!("{} {} {}", face, name, rest),
                None => name.to_string(),
            }
        };
        // 不畫按鈕也不寫提示：Ctrl+] 再按 1/2/3 照樣能餵
        let right = String::new();
        let room = cols.saturating_sub(UnicodeWidthStr::width(right.as_str()) + 2);
        let mut l = String::new();
        let mut used = 0;
        for ch in format!(" {}", left).chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + w > room {
                break;
            }
            used += w;
            l.push(ch);
        }
        let gap = cols.saturating_sub(used + UnicodeWidthStr::width(right.as_str()) + 1);
        (
            format!("{}{}", l, " ".repeat(gap)),
            if UnicodeWidthStr::width(right.as_str()) < cols {
                format!("{} ", right)
            } else {
                String::new()
            },
        )
    }

    /// 畫在真的終端機最後一列（shell 的畫面少一列，碰不到這裡）
    fn draw(&mut self, name: &str, rows: u16, cols: u16, t: f64, force: bool, buf: &mut Vec<u8>) -> bool {
        let (l, r) = self.text(name, cols as usize, t);
        let key = format!("{}{}{}", l, r, self.armed);
        if !force && key == self.drawn {
            return false;
        }
        self.drawn = key;
        buf.extend_from_slice(format!("\x1b[{};1H\x1b[0;48;5;236;38;5;250m{}", rows, l).as_bytes());
        buf.extend_from_slice(format!("{}\x1b[0m", r).as_bytes());
        true
    }
}

/// shell 送出來的「設定捲動範圍」（CSI 上;下 r）改成不包含最後一列，最後一列留給 Bar
fn keep_bar_row(chunk: &[u8], inner: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.len() + 16);
    let mut i = 0;
    while i < chunk.len() {
        if chunk[i] == 0x1b && i + 1 < chunk.len() && chunk[i + 1] == b'[' {
            let mut j = i + 2;
            while j < chunk.len() && (chunk[j].is_ascii_digit() || chunk[j] == b';') {
                j += 1;
            }
            if j < chunk.len() && chunk[j] == b'r' {
                let params = std::str::from_utf8(&chunk[i + 2..j]).unwrap_or("");
                let mut it = params.split(';');
                let top: u16 = it.next().and_then(|x| x.parse().ok()).filter(|&x| x > 0).unwrap_or(1);
                let bot: u16 = it.next().and_then(|x| x.parse().ok()).filter(|&x| x > 0).unwrap_or(inner).min(inner);
                out.extend_from_slice(format!("\x1b[{};{}r", top, bot).as_bytes());
                i = j + 1;
                continue;
            }
        }
        // ESC c（整個重設）會把捲動範圍也重設：補回去
        if chunk[i] == 0x1b && i + 1 < chunk.len() && chunk[i + 1] == b'c' {
            out.extend_from_slice(format!("\x1bc\x1b[1;{}r", inner).as_bytes());
            i += 2;
            continue;
        }
        out.push(chunk[i]);
        i += 1;
    }
    out
}

/// 這段輸出可能把最後一列（Bar）清掉：清畫面、整個重設、切換全螢幕程式的畫面
fn may_clear_bar(chunk: &[u8]) -> bool {
    chunk_erases(chunk)
        || chunk.windows(2).any(|w| w == b"\x1bc")
        || chunk.windows(5).any(|w| w == b"?1049" || w == b"?1047")
        || chunk.windows(4).any(|w| w == b"[?47")
}

enum Ev {
    Out(Vec<u8>),
    End,
    Armed(bool),
    Feed(&'static str),
    Fed(Result<String, String>),
}

pub fn run(name: &str, pane_path: &Path) -> std::io::Result<i32> {
    use std::io::{Read, Write};
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::sync::mpsc;

    if std::env::var_os("NEKO_SHELL").is_some() {
        eprintln!("已經在 neko shell 裡了。");
        return Ok(1);
    }
    // shell 的畫面比終端機少一列：最後一列是 Bar
    let inner_rows = |r: u16| r.saturating_sub(1).max(1);
    // 開 pty
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if master < 0 || unsafe { libc::grantpt(master) } != 0 || unsafe { libc::unlockpt(master) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let slave_name = unsafe { std::ffi::CStr::from_ptr(libc::ptsname(master)) }.to_string_lossy().into_owned();
    let (rows, cols) = winsize(0);
    let ws = libc::winsize {
        ws_row: inner_rows(rows),
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &ws) };

    let sh = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let slave = std::fs::OpenOptions::new().read(true).write(true).open(&slave_name)?;
    let mut child = unsafe {
        std::process::Command::new(&sh)
            .env("NEKO_SHELL", "1")
            .stdin(slave.try_clone()?)
            .stdout(slave.try_clone()?)
            .stderr(slave)
            .pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
            .spawn()?
    };
    let master_fd = unsafe { OwnedFd::from_raw_fd(master) };
    let mut to_shell = std::fs::File::from(master_fd.try_clone()?);
    let mut from_shell = std::fs::File::from(master_fd);

    crossterm::terminal::enable_raw_mode()?;
    let (tx, rx) = mpsc::channel::<Ev>();
    // 鍵盤 → shell。Ctrl+] 之後的下一個鍵是給貓的：1 乾糧、2 濕食、3 零食；再按一次 Ctrl+] 送出真的 Ctrl+]
    let keys = tx.clone();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        let mut armed = false;
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut pass = Vec::with_capacity(n);
            for &b in &buf[..n] {
                if armed {
                    armed = false;
                    let _ = keys.send(Ev::Armed(false));
                    match b {
                        b'1'..=b'3' => {
                            let _ = keys.send(Ev::Feed(FOODS[(b - b'1') as usize].0));
                        }
                        0x1d => pass.push(0x1d),
                        _ => {} // Esc 或其他鍵：取消
                    }
                } else if b == 0x1d {
                    armed = true;
                    let _ = keys.send(Ev::Armed(true));
                } else {
                    pass.push(b);
                }
            }
            if !pass.is_empty() && to_shell.write_all(&pass).is_err() {
                break;
            }
        }
    });
    // shell → 主迴圈
    let out_tx = tx.clone();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 65536];
        loop {
            match from_shell.read(&mut buf) {
                Ok(n) if n > 0 => {
                    if out_tx.send(Ev::Out(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = out_tx.send(Ev::End);
                    break;
                }
            }
        }
    });

    // 一次送出：終端機支援的話整批畫完才顯示（不支援的會忽略），畫的時候游標先藏起來
    const SYNC_ON: &[u8] = b"\x1b[?2026h\x1b[?25l";
    const SYNC_OFF: &[u8] = b"\x1b[?2026l";

    let mut out = std::io::stdout().lock();
    let mut vt = vt100::Parser::new(inner_rows(rows), cols, 0);
    let mut heat = vec![0.0f64; cols as usize];
    let mut size = (rows, cols);
    let mut cat = Overlay {
        pos: None,
        travel: None,
        h: 0,
        k: 1.0,
        drawn: Default::default(),
        next_wander: now() + 10.0,
    };
    let mut bar = Bar {
        armed: false,
        msg: String::new(),
        msg_until: 0.0,
        feeding: false,
        drawn: String::new(),
    };
    let mut quiet_until = 0.0; // 大量輸出時先不要畫貓，等輸出停一下
    let mut last_frame = 0.0;
    // 等 shell 指令跑完：前景是別的程式（不是 shell 自己）超過 SHELL_NAP 秒，貓就睡著，跑完醒來
    const SHELL_NAP: f64 = 10.0;
    let shell_pid = child.id() as libc::pid_t;
    let mut busy_since: Option<f64> = None;
    let mut dozing = false;
    let mut last_fg_check = 0.0;
    // 從乾淨的畫面開始（貓要知道哪裡是空白），最後一列不讓 shell 捲到
    {
        let mut b = format!("\x1b[0m\x1b[2J\x1b[1;{}r\x1b[H", inner_rows(rows)).into_bytes();
        bar.draw(name, rows, cols, now(), true, &mut b);
        b.extend_from_slice(b"\x1b[H");
        out.write_all(&b)?;
        out.flush()?;
    }
    loop {
        let got = rx.recv_timeout(std::time::Duration::from_millis(40));
        let mut ended = false;
        let mut chunk = Vec::new();
        let handle = |ev: Ev, chunk: &mut Vec<u8>, ended: &mut bool, bar: &mut Bar| match ev {
            Ev::Out(b) => chunk.extend(b),
            Ev::End => *ended = true,
            Ev::Armed(a) => bar.armed = a,
            Ev::Feed(food) => {
                if !bar.feeding {
                    bar.feeding = true;
                    let (tx, pane) = (tx.clone(), pane_path.to_path_buf());
                    std::thread::spawn(move || {
                        let _ = tx.send(Ev::Fed(feed(food, &pane)));
                    });
                }
            }
            Ev::Fed(r) => {
                bar.feeding = false;
                bar.msg = r.unwrap_or_else(|e| e);
                bar.msg_until = now() + 4.0;
            }
        };
        match got {
            Ok(ev) => handle(ev, &mut chunk, &mut ended, &mut bar),
            Err(mpsc::RecvTimeoutError::Disconnected) => ended = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Ok(ev) = rx.try_recv() {
            handle(ev, &mut chunk, &mut ended, &mut bar);
        }
        let t = now();
        let mut buf: Vec<u8> = Vec::new();
        let mut redraw_bar = false;
        // 視窗大小變了：告訴 shell；終端機會自己重排，舊的貓不用還原
        let now_size = winsize(0);
        if now_size != size {
            size = now_size;
            let inner = inner_rows(size.0);
            let ws = libc::winsize {
                ws_row: inner,
                ws_col: size.1,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &ws) };
            vt.set_size(inner, size.1);
            heat = vec![0.0; size.1 as usize];
            cat.drawn.clear();
            cat.pos = None;
            cat.travel = None;
            // 捲動範圍跟著新的高度；設定捲動範圍會把游標移到左上，之後放回 shell 的游標
            buf.extend_from_slice(format!("\x1b[1;{}r", inner).as_bytes());
            buf.extend_from_slice(&vt.screen().cursor_state_formatted());
            redraw_bar = true;
        }
        if !chunk.is_empty() {
            let inner = inner_rows(size.0);
            let old = vt.screen().clone();
            vt.process(&chunk);
            let new = vt.screen();
            let shown = keep_bar_row(&chunk, inner);
            if may_clear_bar(&chunk) {
                redraw_bar = true;
            }
            // 只有這段輸出真的碰到貓（寫進那幾格、捲動、游標走進去）才讓位；打字、改字不會動到貓
            // 讓位的原因：捲動（貓會被捲走）、字寫進貓的格子、或貓的格子被清掉（例如 zsh 重畫提示字元時清除游標以下）
            let scrolled = !cat.drawn.is_empty() && may_scroll(&chunk, &old, new);
            let overlap = cat.drawn.keys().any(|&(r, c)| !vt_blank(new, r, c));
            let touched = !cat.drawn.is_empty()
                && (scrolled
                    || overlap
                    || chunk_erases(&chunk)
                    || cat.drawn.keys().any(|&(r, c)| {
                        old.cell(r as u16, c as u16).map(|x| (x.contents(), x.bgcolor(), x.inverse()))
                            != new.cell(r as u16, c as u16).map(|x| (x.contents(), x.bgcolor(), x.inverse()))
                    }));
            if touched {
                buf.extend_from_slice(SYNC_ON);
                cat.erase(&old, &mut buf); // 還原成輸出之前的樣子，輸出才會接得上
                if scrolled || overlap {
                    cat.bumped(if scrolled { "scroll" } else { "overlap" });
                    // 被捲動或蓋到：這一格不畫，等下一格重新找位置
                    cat.pos = cat.pos.filter(|_| !scrolled);
                }
                buf.extend_from_slice(&old.attributes_formatted());
                buf.extend_from_slice(&old.cursor_state_formatted());
                buf.extend_from_slice(&shown);
                if chunk.len() < 4096 {
                    // 少量輸出（按 Enter、一兩行）：同一批就把貓畫到新位置，不會閃
                    let mut more = Vec::new();
                    cat.frame(new, &heat, t, name, pane_path, &mut more);
                    if !more.is_empty() {
                        buf.extend_from_slice(&more);
                        buf.extend_from_slice(&new.attributes_formatted());
                        buf.extend_from_slice(&new.cursor_state_formatted());
                    }
                    last_frame = t;
                } else {
                    quiet_until = t + 0.15;
                }
                buf.extend_from_slice(SYNC_OFF);
            } else {
                buf.extend_from_slice(&shown);
                if chunk.len() >= 4096 {
                    quiet_until = t + 0.15;
                }
            }
            // 記哪幾欄常常有字
            let (r, c) = new.size();
            for (x, h) in heat.iter_mut().enumerate().take(c as usize) {
                let used = (0..r as usize).filter(|&y| !vt_blank(new, y, x)).count() as f64 / r as f64;
                *h = *h * 0.97 + used * 0.03;
            }
        }
        if ended {
            out.write_all(&buf)?;
            break;
        }
        // 有沒有指令在跑（每半秒看一次）；vim、less 這種全螢幕程式是你在用，不算在等
        if t - last_fg_check >= 0.5 {
            last_fg_check = t;
            let fg = unsafe { libc::tcgetpgrp(master) };
            let waiting = fg > 0 && fg != shell_pid && !vt.screen().alternate_screen();
            if waiting {
                let since = *busy_since.get_or_insert(t);
                if !dozing && t - since > SHELL_NAP {
                    dozing = true;
                    let _ = act("doze", SHELL_W, cat.k, name, pane_path);
                }
            } else {
                busy_since = None;
                if dozing {
                    dozing = false;
                    let _ = act("wake", SHELL_W, cat.k, name, pane_path);
                }
            }
        }
        // 最下面那一列：內容變了（營養狀態、按了 Ctrl+]、餵完的訊息）或被清掉了就重畫
        {
            let mut b = Vec::new();
            if bar.draw(name, size.0, size.1, t, redraw_bar, &mut b) {
                let scr = vt.screen();
                buf.extend_from_slice(SYNC_ON);
                buf.extend_from_slice(&b);
                buf.extend_from_slice(&scr.attributes_formatted());
                buf.extend_from_slice(&scr.cursor_state_formatted());
                buf.extend_from_slice(SYNC_OFF);
            }
        }
        if !buf.is_empty() {
            out.write_all(&buf)?;
            out.flush()?;
            buf.clear();
        }
        if t < quiet_until || t - last_frame < 0.08 {
            continue;
        }
        last_frame = t;
        let scr = vt.screen();
        let mut more = Vec::new();
        cat.frame(scr, &heat, t, name, pane_path, &mut more);
        if !more.is_empty() {
            buf.extend_from_slice(SYNC_ON);
            buf.extend_from_slice(&more);
            buf.extend_from_slice(&scr.attributes_formatted());
            buf.extend_from_slice(&scr.cursor_state_formatted());
            buf.extend_from_slice(SYNC_OFF);
            out.write_all(&buf)?;
            out.flush()?;
        }
    }
    if dozing {
        let _ = act("wake", SHELL_W, cat.k, name, pane_path);
    }
    // 離開：捲動範圍還原、清掉最後一列
    let _ = write!(out, "\x1b[r\x1b[{};1H\x1b[0m\x1b[2K", size.0);
    let _ = out.flush();
    let _ = crossterm::terminal::disable_raw_mode();
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(0);
    Ok(code)
}
