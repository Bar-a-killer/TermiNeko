//! 跟 Python 核心（neko-core）講話。
//!
//! 營養、心情、體重、紀錄的計算和存檔全部在 Python（neko_api/）；Rust 這邊只拿結果來畫畫面。找 neko-core 的順序：環境變數 NEKO_CORE → 這個 repo 的 .venv/bin/neko-core → PATH。

use serde_json::Value;
use std::process::{Command, Stdio};

/// 啟動 neko-core 的指令
fn command() -> Command {
    if let Ok(c) = std::env::var("NEKO_CORE") {
        let mut parts = c.split_whitespace();
        let mut cmd = Command::new(parts.next().unwrap_or("neko-core"));
        cmd.args(parts);
        return cmd;
    }
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv/bin/neko-core");
    if venv.exists() {
        return Command::new(venv);
    }
    Command::new("neko-core")
}

const MISSING: &str = "找不到 neko-core（Python 核心）。在 repo 裡跑 make venv，或設定 NEKO_CORE。";

/// 跑一次 neko-core（例如 `once(&["line"])`），回傳 stdout
pub fn once(args: &[&str]) -> Result<String, String> {
    let out = command().args(args).stderr(Stdio::inherit()).output().map_err(|_| MISSING.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string()))
            .unwrap_or(text))
    }
}

/// 貓的名字（畫面上的說明用）；問不到就叫「貓」
pub fn name() -> String {
    once(&["status"])
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("cat")?.get("name")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "貓".to_string())
}
