//! 外觀資料：顏色、明暗、小圖、姿勢骨架、姿勢轉換。
//!
//! 來源是 assets/cat.json，編譯時嵌入執行檔。設定 NEKO_ASSETS=<檔案> 時，於啟動時讀取該檔案，
//! 逐層合併到內建資料上（物件逐鍵合併，其他值直接取代），因此覆蓋檔只需包含要修改的項目。
//! 覆蓋檔無法讀取、格式錯誤或內容不完整時，輸出一次警告並使用內建資料。

use serde_json::{Map, Value};
use std::sync::OnceLock;

const EMBEDDED: &str = include_str!("../assets/cat.json");

/// 姿勢名稱，順序與 pix::ALL 一致
pub(crate) const POSES: [&str; 12] = [
    "stand", "sit", "loaf", "sleep", "eat", "crouch", "air", "stretch", "groom", "yawn", "happy", "meow",
];

/// 姿勢欄位與其數值個數（向量 2、純量 1），順序與 Pose::arr() 一致，合計 26
const FIELDS: [(&str, usize); 16] = [
    ("hip", 2),
    ("hip_r", 1),
    ("chest", 2),
    ("chest_r", 1),
    ("head", 2),
    ("head_r", 1),
    ("tilt", 1),
    ("fnp", 2),
    ("ffp", 2),
    ("bnp", 2),
    ("bfp", 2),
    ("hock", 2),
    ("tail_c", 2),
    ("tail_t", 2),
    ("eye", 1),
    ("mouth", 1),
];
pub(crate) const POSE_LEN: usize = 26;

/// 未定義顏色（透明）
pub(crate) const NONE: u32 = 0x0100_0000;

pub(crate) struct Sprite {
    pub dx: i32,
    pub top: i32,
    pub rows: Vec<String>,
}

pub(crate) struct Assets {
    /// 顏色字元（ASCII）→ 0xRRGGBB；未定義為 NONE
    palette: [u32; 128],
    /// 顏色字元 → 亮度 0～3 對應的顏色字元
    shade: [[char; 4]; 128],
    pub sprites: std::collections::HashMap<String, Sprite>,
    /// 依 POSES 順序
    pub poses: Vec<[f64; POSE_LEN]>,
    /// 依 POSES 順序：[(目標姿勢的索引, 秒數)]
    pub edges: Vec<Vec<(usize, f64)>>,
}

impl Assets {
    pub fn color(&self, c: char) -> u32 {
        self.palette.get(c as usize).copied().unwrap_or(NONE)
    }
    pub fn shade(&self, c: char, tone: u8) -> char {
        match self.shade.get(c as usize) {
            Some(row) => row[(tone as usize).min(3)],
            None => c,
        }
    }
}

pub(crate) fn get() -> &'static Assets {
    static A: OnceLock<Assets> = OnceLock::new();
    A.get_or_init(load)
}

fn load() -> Assets {
    let base: Value = serde_json::from_str(EMBEDDED).expect("assets/cat.json 格式錯誤");
    if let Some(path) = std::env::var_os("NEKO_ASSETS") {
        let over = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
            .and_then(|o| {
                let mut merged = base.clone();
                merge(&mut merged, o);
                build(&merged)
            });
        match over {
            Ok(a) => return a,
            Err(e) => eprintln!("neko: 忽略 NEKO_ASSETS（{}）：{}", path.to_string_lossy(), e),
        }
    }
    build(&base).expect("assets/cat.json 內容錯誤")
}

/// 物件逐鍵合併，其他值直接取代
fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, v) => *slot = v,
    }
}

fn obj<'a>(v: &'a Value, key: &str) -> Result<&'a Map<String, Value>, String> {
    v.get(key).and_then(Value::as_object).ok_or_else(|| format!("缺少 {}", key))
}

fn one_char(k: &str, what: &str) -> Result<char, String> {
    let mut it = k.chars();
    match (it.next(), it.next()) {
        (Some(c), None) if c.is_ascii() && c != '.' && c != ' ' => Ok(c),
        _ => Err(format!("{} 的鍵 \"{}\" 必須是單一 ASCII 字元（不可為 '.' 或空白）", what, k)),
    }
}

fn build(root: &Value) -> Result<Assets, String> {
    // 顏色
    let mut palette = [NONE; 128];
    for (k, v) in obj(root, "palette")? {
        let c = one_char(k, "palette")?;
        let hex = v
            .get("color")
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix('#'))
            .filter(|s| s.len() == 6)
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .ok_or_else(|| format!("palette.{}.color 必須是 #rrggbb", k))?;
        palette[c as usize] = hex;
    }

    // 明暗：未列出的顏色不變；不足 4 個時延用最後一個
    let mut shade = [[' '; 4]; 128];
    for (i, row) in shade.iter_mut().enumerate() {
        *row = [i as u8 as char; 4];
    }
    for (k, v) in obj(root, "shade")? {
        let c = one_char(k, "shade")?;
        let list: Vec<char> = v
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().and_then(|s| one_char(s, "shade").ok())).collect())
            .unwrap_or_default();
        if list.is_empty() || list.len() > 4 {
            return Err(format!("shade.{} 必須是 1～4 個顏色字元", k));
        }
        for (t, slot) in shade[c as usize].iter_mut().enumerate() {
            *slot = list[t.min(list.len() - 1)];
        }
    }

    // 小圖
    let mut sprites = std::collections::HashMap::new();
    for (k, v) in obj(root, "sprites")? {
        let int = |f: &str| {
            v.get(f)
                .and_then(Value::as_i64)
                .map(|n| n as i32)
                .ok_or_else(|| format!("sprites.{}.{} 必須是整數", k, f))
        };
        let rows = v
            .get("rows")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().map(|r| r.as_str().map(String::from)).collect::<Option<Vec<_>>>())
            .ok_or_else(|| format!("sprites.{}.rows 必須是字串陣列", k))?;
        sprites.insert(
            k.clone(),
            Sprite {
                dx: int("dx")?,
                top: int("top")?,
                rows,
            },
        );
    }
    for need in ["food_full", "food_half", "food_low", "bowl", "ball"] {
        if !sprites.contains_key(need) {
            return Err(format!("缺少 sprites.{}", need));
        }
    }

    // 姿勢
    let poses_obj = obj(root, "poses")?;
    let mut poses = Vec::with_capacity(POSES.len());
    for name in POSES {
        let fields = resolve(poses_obj, name, 0)?;
        let mut arr = [0.0; POSE_LEN];
        let mut i = 0;
        for (f, n) in FIELDS {
            let v = fields.get(f).ok_or_else(|| format!("poses.{} 缺少 {}", name, f))?;
            let nums: Vec<f64> = if n == 1 {
                v.as_f64().into_iter().collect()
            } else {
                v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()
            };
            if nums.len() != n || nums.iter().any(|x| !x.is_finite()) {
                return Err(format!("poses.{}.{} 必須是{}", name, f, if n == 1 { "數字" } else { "兩個數字的陣列" }));
            }
            arr[i..i + n].copy_from_slice(&nums);
            i += n;
        }
        poses.push(arr);
    }

    // 姿勢轉換
    let edges_obj = obj(root, "edges")?;
    let index = |n: &str| POSES.iter().position(|p| *p == n);
    let mut edges = Vec::with_capacity(POSES.len());
    for name in POSES {
        let list = edges_obj.get(name).and_then(Value::as_array).ok_or_else(|| format!("缺少 edges.{}", name))?;
        let mut out = vec![];
        for e in list {
            let pair = e.as_array().filter(|a| a.len() == 2);
            let to = pair.and_then(|a| a[0].as_str()).and_then(index);
            let secs = pair.and_then(|a| a[1].as_f64()).filter(|s| s.is_finite() && *s > 0.0);
            match (to, secs) {
                (Some(t), Some(s)) => out.push((t, s)),
                _ => return Err(format!("edges.{} 的項目必須是 [已知姿勢名稱, 正數秒數]", name)),
            }
        }
        edges.push(out);
    }

    Ok(Assets {
        palette,
        shade,
        sprites,
        poses,
        edges,
    })
}

/// 展開 base：先取 base 姿勢的所有欄位，再以自己的欄位覆蓋
fn resolve(poses: &Map<String, Value>, name: &str, depth: usize) -> Result<Map<String, Value>, String> {
    if depth > POSES.len() {
        return Err(format!("poses.{} 的 base 形成循環", name));
    }
    let own = poses.get(name).and_then(Value::as_object).ok_or_else(|| format!("缺少 poses.{}", name))?;
    let mut out = match own.get("base") {
        Some(b) => {
            let b = b.as_str().ok_or_else(|| format!("poses.{}.base 必須是姿勢名稱", name))?;
            resolve(poses, b, depth + 1)?
        }
        None => Map::new(),
    };
    for (k, v) in own {
        if k != "base" {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_are_valid() {
        let root: Value = serde_json::from_str(EMBEDDED).unwrap();
        let a = build(&root).unwrap();
        assert_eq!(a.color('o'), 0xf2a250);
        assert_eq!(a.color('.'), NONE);
        assert_eq!(a.shade('o', 0), 'l');
        assert_eq!(a.shade('d', 3), 'D');
        assert_eq!(a.shade('k', 2), 'k');
        assert_eq!(a.poses.len(), POSES.len());
    }

    #[test]
    fn override_merges_partially() {
        let mut root: Value = serde_json::from_str(EMBEDDED).unwrap();
        merge(
            &mut root,
            serde_json::json!({ "palette": { "o": { "color": "#000000" } }, "poses": { "sit": { "tilt": 0.5 } } }),
        );
        let a = build(&root).unwrap();
        assert_eq!(a.color('o'), 0x000000);
        assert_eq!(a.color('k'), 0x3a2618);
        let sit = POSES.iter().position(|p| *p == "sit").unwrap();
        assert_eq!(a.poses[sit][9], 0.5); // tilt
                                          // meow 以 sit 為 base，但自己定義了 tilt
        let meow = POSES.iter().position(|p| *p == "meow").unwrap();
        assert_eq!(a.poses[meow][9], 0.3);
    }

    #[test]
    fn invalid_override_is_rejected() {
        let mut root: Value = serde_json::from_str(EMBEDDED).unwrap();
        merge(&mut root, serde_json::json!({ "palette": { "o": { "color": "orange" } } }));
        assert!(build(&root).is_err());
    }
}
