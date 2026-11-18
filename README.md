# neko 🐈 — 住在終端機裡的貓

一隻用真實營養學規則養的貓。時間是真的在走：你不餵牠，牠會餓、會瘦；餵太多，牠會胖。
牠住在你的終端機空白處，也會在 Claude Code 的輸入框上方陪 Claude 工作。

| 部分 | 語言 | 做什麼 |
|---|---|---|
| `neko_api/` | Python | **所有計算和紀錄**：營養模型、心情、體重、紀錄、存檔（`state.json`）、`neko-core` 指令、HTTP API、體重模擬 |
| `src/` | Rust | **只有畫面和終端機互動**：像素貓動畫、`neko shell`、Claude Code 外掛用的畫面 |
| `mod/` | TypeScript | Claude Code 外掛：輸入框上方的貓 |

Rust 不自己算任何營養，都透過 `neko-core` 問 Python。所以終端機、Claude Code、HTTP API 看到的永遠是同一隻貓，所有寫入都經過同一個檔案鎖。

---

## 快速開始

需要 [Rust](https://rustup.rs/) 和 Python 3.10 以上（有 [uv](https://github.com/astral-sh/uv) 會自動用它）。

```bash
make            # 列出所有指令
make shell      # 開一個 shell，貓住在終端機的空白處（第一次會自動編譯、建 .venv）
make install    # 裝到 ~/.local/bin，之後在哪裡都能直接打 neko shell
```

新領養的貓預設叫 **TCSG12**，改名：`.venv/bin/neko-core rename 小黑`。

---

## 用法

### neko shell：貓住在終端機的空白處

```bash
neko shell        # 或 make shell；exit 離開
```

- 終端機照常使用。貓只出現在現在空白、而且平常很少被寫到的地方（多半是右下角）。
- 輸出一碰到貓那塊，那塊立刻還給你；不急的時候（例如游標靠近）貓會走過去、跳過去換位置。
- vim、less 這類全螢幕程式執行時貓先躲起來。
- 最下面一列是貓的狀態：`(^.^) TCSG12 3.99kg 肚子47%`。
- 餵食：按 **Ctrl+]** 再按 **1** 乾糧、**2** 濕食、**3** 零食（畫面上不顯示按鈕；連按兩次 Ctrl+] 送出真的 Ctrl+]）。

### Claude Code 外掛

`mod/` 讓貓在 Claude 工作時陪在輸入框上方：看你打字、陪 Claude 改檔、指令失敗嚇一跳、做完叫你。

1. `make build`
2. 把 `mod/` 加進 Claude Code 的外掛資料夾，例如設定 `CLAUDE_CODE_PLUGIN_DIRS=<repo>/mod`

外掛會執行 `<repo>/target/release/neko`；放在別的地方的話設定 `NEKO_BIN`。

| 指令 | 做什麼 |
|---|---|
| `/neko 乾糧`、`/neko 濕食`、`/neko 零食` | 餵食（英文 `dry`、`wet`、`treat` 也可以） |
| `/neko 摸摸`、`丟球`、`叫牠`、`跳`、`哄睡`、`吃飯` | 讓貓做動作（只有動畫，不影響營養） |
| `/neko 收起`、`/neko 出來` | 把貓藏起來 / 叫出來 |
| `/neko 大小 1.5` | 調整解析度（字調小之後可以調大，貓畫得更細） |

### 其他

```bash
neko              # 全螢幕的貓（按 q 離開）
neko line         # 一行狀態，給 tmux 狀態列：set -g status-right '#(neko line)'
neko feed wet     # 餵食（dry 乾糧 10 克、wet 濕食 40 克、treat 零食 4 克）
```

### 貓什麼時候會睡覺

貓不會自己睡，只有在「等」的時候才會：

| 什麼時候 | 哪裡 | 什麼時候醒 |
|---|---|---|
| Claude 這一輪做了超過 45 秒 | Claude Code | 這一輪結束 |
| Claude 跑的 shell 指令超過 10 秒還沒好 | Claude Code | 指令跑完 |
| 你在 `neko shell` 跑的指令超過 10 秒還沒好（vim、less 不算） | neko shell | 指令跑完 |
| `/neko 哄睡` | Claude Code | 睡 20 秒左右 |

動畫狀態存在 `pane.json`，neko shell 和 Claude Code 共用，所以兩邊的貓會一起睡、一起醒。

---

## 規則（營養模型）

| 項目 | 公式 / 數值 |
|---|---|
| 每日熱量需求 | `70 × 體重(kg)^0.75 × 1.2`（RER × 結紮成貓係數） |
| 體重變化 | 每多吃或少吃 7000 kcal，體重 ±1 kg |
| 肚子容量 | 最多存一天份，吃超過的直接變成體重 |
| 理想體重 | 4.0 kg；體重下限 2.88 kg（太久沒回來會有鄰居幫忙餵） |
| 體態分數 BCS | 1–9，5 為理想，每偏離理想體重 10% 差一級 |
| 心情 | 每小時 −1.5；摸摸 +3、玩球 +18、零食 +10、吃飽 +6、吃太撐 −5 |

| 食物 | kcal/kg | 預設份量 |
|---|---|---|
| 乾糧 `dry` | 3800 | 10 g |
| 濕食 `wet` | 1000 | 40 g |
| 零食 `treat` | 3750 | 4 g |

`mood` 可能是：`starving`、`hungry`、`full`、`bored`、`happy`、`ok`。

---

## Python 核心

### neko-core

終端機版靠這個指令跟 Python 講話，你也可以自己用：

```bash
neko-core status          # 狀態（JSON）
neko-core line            # (^.^) TCSG12 3.99kg 肚子17%
neko-core feed wet 30     # 餵 30 克濕食
neko-core pet | play      # 摸摸、丟球
neko-core rename 小黑     # 改名（最多 12 字）
neko-core log 5           # 最近 5 筆紀錄
neko-core adopt [名字]    # 重新領養（目前這隻會離開）
```

成功時印 `{"ok": true, "message": ..., "cat": {...}}`，失敗時印 `{"ok": false, "error": ...}` 並以 1 結束。

### HTTP API

```bash
make api                        # http://127.0.0.1:8000，文件在 /docs（Swagger UI）
PORT=9000 make api
```

| 方法 | 路徑 | 說明 |
|---|---|---|
| `GET` | `/cat` | 貓現在的狀態 |
| `GET` | `/cat/log?limit=20` | 最近的紀錄 |
| `POST` | `/cat/feed` | 餵食 `{"food": "dry" \| "wet" \| "treat", "grams": 30}`（grams 可省略） |
| `POST` | `/cat/pet` | 摸摸 |
| `POST` | `/cat/play` | 丟球；貓餓壞了會回 `409` |
| `POST` | `/cat/rename` | 改名 `{"name": "小黑"}`（最多 12 字） |
| `POST` | `/cat/advance` | 快轉時間 `{"hours": 6}`（測試用） |
| `POST` | `/cat/adopt` | 重新領養 `{"name": "新貓", "confirm": true}` |
| `GET` | `/foods` | 食物種類與熱量 |
| `GET` | `/nutrition?weight=4.5` | 某個體重一天該吃多少（kcal、乾糧克數、濕食克數） |
| `POST` | `/simulate` | 模擬固定餵法下的體重變化，**不影響存檔** |

```bash
$ curl -s -X POST localhost:8000/cat/feed -H 'Content-Type: application/json' -d '{"food": "wet"}'
{"message":"TCSG12 吃了 40 克（40 kcal）","cat":{"name":"TCSG12","weight_kg":3.986,"bcs":5,"mood":"ok","face":"o.o", ...}}

$ curl -s -X POST localhost:8000/simulate -H 'Content-Type: application/json' \
       -d '{"start_weight": 4, "daily_kcal": 280, "days": 90, "jitter": 0.1, "seed": 1}'
[{"day":0,"w":4.0,"bcs":5,"need":237.6}, ..., {"day":90,"w":4.3974,"bcs":6,"need":255.1}]
```

API 預設只聽 `127.0.0.1`，而且沒有驗證；要對外開放的話請自己加上驗證。

### 在 Python 裡直接用

```python
from neko_api.model import FOODS, need, simulate
from neko_api.store import Store

print(need(4.0))                      # 237.6 kcal/天

with Store().edit() as cat:           # 讀檔 → 算到現在 → 修改 → 存回
    print(cat.feed(FOODS["dry"], 15))

points = simulate(start_w=4.0, daily_kcal=260, days=180)
```

---

## 開發

```bash
make test       # Python 測試
make fmt        # 整理格式（rustfmt、ruff）
make check      # 全部檢查一次：格式、clippy、ruff、測試、外掛驗證
make clean      # 刪掉編譯產物和快取
```

| 環境變數 | 用途 |
|---|---|
| `NEKO_STATE` | 存檔位置（預設 `~/neko/state.json`；`pane.json` 等動畫狀態放在同一個資料夾） |
| `NEKO_CORE` | `neko-core` 在哪（預設依序找這個 repo 的 `.venv/bin/neko-core`、`PATH`） |
| `NEKO_BIN` | Claude Code 外掛用的 `neko` 執行檔（預設 `<repo>/target/release/neko`） |
| `NEKO_LOG` | neko shell 把貓換位置的原因寫到這個檔案（除錯用） |

### 專案結構

```
.
├── neko_api/            Python：所有計算和紀錄
│   ├── model.py         營養模型、心情、吃飯的規則 + 模擬
│   ├── store.py         讀寫 state.json（原子寫入 + 檔案鎖）
│   ├── core.py          每個操作（餵食、摸摸、改名…）和狀態格式
│   ├── cli.py           neko-core 指令
│   └── server.py        FastAPI 路由
├── src/                 Rust：只有畫面和終端機互動
│   ├── main.rs          指令列
│   ├── core.rs          呼叫 neko-core
│   ├── pix.rs           像素畫的貓：姿勢、動作劇本、畫圖、全螢幕版
│   └── shell.rs         neko shell：pty、找空白處、讓位、狀態列
├── mod/                 Claude Code 外掛（hooks/register.tsx）
├── tests/               pytest
├── Makefile
├── Cargo.toml
└── pyproject.toml
```

## 授權

[MIT](LICENSE)
