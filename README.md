# neko 🐈 — 住在終端機裡的貓

一隻用真實營養學規則養的貓。時間是真的在走：你不餵牠，牠會餓、會瘦；餵太多，牠會胖。

這個專案有兩個部分，分工很清楚：

| 部分 | 語言 | 做什麼 |
|---|---|---|
| `neko_api/` | Python | **所有計算和紀錄**：營養模型、心情、體重、紀錄、存檔（`state.json`），還有 HTTP API 和體重模擬 |
| `src/` | Rust | **只有畫面和終端機互動**：像素貓動畫、`neko shell`、Claude Code 外掛的畫面 |

Rust 不自己算任何營養：它透過 `neko-core` 指令問 Python 貓的狀態。餵食、摸摸這些動作用 `neko-core` 或 HTTP API。
所以 API、終端機、Claude Code 外掛看到的永遠是同一隻貓，所有寫入都經過同一個檔案鎖。

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

---

## 快速開始

```bash
make            # 列出所有指令
make shell      # 開一個 shell，貓住在終端機的空白處（會自動編譯）
make install    # 裝到 ~/.local/bin，之後在哪裡都能直接打 neko shell
make api        # 啟動 HTTP API（第一次會自動建 .venv）
make test       # 跑測試
```

## Python API

### 安裝

需要 Python 3.10 以上。

```bash
python -m venv .venv
source .venv/bin/activate
pip install -e '.[dev]'
```

（用 [uv](https://github.com/astral-sh/uv) 的話：`uv venv && uv pip install -e '.[dev]'`）

### 啟動

```bash
neko-api                    # http://127.0.0.1:8000
neko-api --port 9000 --reload
```

啟動後打開 **http://127.0.0.1:8000/docs**，可以直接在網頁上試每個 API（Swagger UI）。

存檔位置預設是 `~/neko/state.json`，可以用環境變數換掉：

```bash
NEKO_STATE=/tmp/test-cat.json neko-api
```

### 接口一覽

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

### 範例

```bash
$ curl -s localhost:8000/cat
{"name":"芝麻","weight_kg":3.989,"ideal_kg":4.0,"bcs":5,"need_kcal":237.1,
 "belly_kcal":0.0,"belly_pct":0.0,"joy":54.2,"mood":"starving","face":">.<",
 "fed_today_kcal":0.0,"age_days":0.88,"bowl_g":0.0}

$ curl -s -X POST localhost:8000/cat/feed -H 'Content-Type: application/json' \
       -d '{"food": "wet"}'
{"message":"芝麻 吃了 40 克（40 kcal）","cat":{ ... "mood":"happy","face":"^.^" ... }}

$ curl -s -X POST localhost:8000/simulate -H 'Content-Type: application/json' \
       -d '{"start_weight": 4, "daily_kcal": 280, "days": 90, "jitter": 0.1, "seed": 1}'
[{"day":0,"w":4.0,"bcs":5,"need":237.6}, ..., {"day":90,"w":4.3974,"bcs":6,"need":255.1}]
```

`mood` 可能是：`starving`、`hungry`、`full`、`sleep`、`bored`、`happy`、`ok`。

### neko-core（給終端機版用的指令）

Rust 終端機版就是靠這個指令跟 Python 講話；你也可以自己用：

```bash
neko-core status          # 狀態（JSON）
neko-core line            # (^.^) 芝麻 3.99kg 肚子17%
neko-core feed wet 30     # 餵 30 克濕食
neko-core pet | play      # 摸摸、丟球
neko-core log 5           # 最近 5 筆紀錄
```

完整的操作列表在 `neko_api/core.py`。

### 在 Python 裡直接用

```python
from neko_api.model import Cat, FOODS, need, simulate
from neko_api.store import Store

print(need(4.0))                      # 237.6 kcal/天

with Store().edit() as cat:           # 讀檔 → 算到現在 → 修改 → 存回
    print(cat.feed(FOODS["dry"], 15))

points = simulate(start_w=4.0, daily_kcal=260, days=180)
```

### 測試

```bash
pytest
```

---

## Rust 終端機版

需要 [Rust](https://rustup.rs/)，以及上面裝好的 Python 核心（`make venv`）。終端機版會依序找：環境變數 `NEKO_CORE` → 這個 repo 的 `.venv/bin/neko-core` → `PATH` 上的 `neko-core`。

```bash
cargo build --release
./target/release/neko          # 全螢幕的貓（按 q 離開）
./target/release/neko shell    # 開一個 shell，貓住在終端機的空白處（exit 離開）
./target/release/neko line     # 印一行狀態，例如：(^.^) 芝麻 3.99kg 肚子17%
```

`neko shell` 會把你的 shell 包起來，終端機照常使用，貓只出現在現在空白、而且平常很少被寫到的地方（多半是右下角）。輸出一碰到貓那塊，那塊立刻還給你，貓換到別的空地；vim、less 這類全螢幕程式執行時貓會先躲起來。

`mod/` 是 Claude Code 的外掛，讓貓在 Claude 工作時陪在旁邊、看它做什麼。先 `cargo build --release`，再把 `mod/` 加進 Claude Code 的外掛資料夾（例如設定 `CLAUDE_CODE_PLUGIN_DIRS=<repo>/mod`）。外掛預設執行 `<repo>/target/release/neko`；執行檔放在別的地方的話，設定環境變數 `NEKO_BIN` 指過去。

---

## 專案結構

```
.
├── neko_api/            Python：所有計算和紀錄
│   ├── model.py         營養模型、心情、吃飯的規則 + 模擬
│   ├── store.py         讀寫 state.json（原子寫入 + 檔案鎖）
│   ├── core.py          每個操作（餵食、吃一口、摸摸…）和狀態格式，neko-core 和 API 共用
│   ├── cli.py           neko-core 指令（單次 / 常駐）
│   └── server.py        FastAPI 路由
├── src/                 Rust：只有畫面和終端機互動
│   ├── main.rs          指令列
│   ├── core.rs          呼叫 neko-core
│   └── pix.rs           像素畫的貓、neko shell
├── tests/               pytest
├── mod/                 Claude Code 外掛
├── Cargo.toml
└── pyproject.toml
```

## 注意事項

- API、neko-core、終端機版可以同時用：所有修改都經過 Python 的檔案鎖，不會互相蓋掉。
- API 預設只聽 `127.0.0.1`，而且沒有驗證；要對外開放的話請自己加上驗證。

## 授權

[MIT](LICENSE)
