//! neko — 住在終端機裡的貓：畫面和終端機互動。
//!
//! 營養、心情、體重、紀錄的計算和存檔都在 Python 核心（neko_api/，透過 neko-core 指令）；
//! 這裡只負責畫貓（全螢幕、neko shell、Claude Code 外掛）。

mod core;
mod pix;


use std::path::PathBuf;

// ───────────────────────── Claude Code 用的貓（畫法在 pix.rs） ─────────────────────────

/// 動畫狀態（pane.json）放在存檔旁邊：NEKO_STATE 那個資料夾，不然 ~/neko
fn pane_path() -> PathBuf {
    if let Ok(p) = std::env::var("NEKO_STATE") {
        return PathBuf::from(p).with_file_name("pane.json");
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join("neko/pane.json")
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.as_str() {
        // 一行狀態：Python 算好的
        "line" => match crate::core::once(&["line"]) {
            Ok(t) => println!("{}", t),
            Err(e) => {
                eprintln!("neko: {}", e);
                std::process::exit(1);
            }
        },
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
            let name = crate::core::name();
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
            let name = crate::core::name();
            match arg.as_str() {
                "raster" => print!("{}", pix::raster(num(2), kf(3), &name, &pane_path())),
                "dump" => print!("{}", pix::dump(num(2), kf(3), &pane_path())),
                "trace" => print!("{}", pix::trace(num(2))),
                _ => println!("{}", pix::act(args.get(2).map(|s| s.as_str()).unwrap_or(""), num(3), kf(4), &name, &pane_path())),
            }
        }
        "" => {
            let name = crate::core::name();
            if let Err(e) = pix::term(&name, &pane_path().with_file_name("pane-term.json")) {
                eprintln!("neko: {}", e);
                std::process::exit(1);
            }
        }
        // 把 shell 包起來，貓住在終端機的空白處；exit 離開
        "shell" => {
            let name = crate::core::name();
            match pix::shell(&name, &pane_path()) {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    let _ = crossterm::terminal::disable_raw_mode();
                    eprintln!("neko: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            println!(
                "neko                         全螢幕的貓（字越小畫得越細，按 q 離開）\n\
                 neko shell                   開一個 shell，貓住在終端機的空白處（exit 離開）\n\
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
