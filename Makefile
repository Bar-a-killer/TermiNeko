# neko 建置與執行指令。執行 make 顯示指令列表。
# Rust（src/）負責畫面繪製與終端機互動；營養、心情、紀錄由 Python 核心（neko_api/，neko-core）處理。
# 執行前需要 .venv，首次執行時自動建立。

BIN    := target/release/neko
PREFIX ?= $(HOME)/.local
VENV   := .venv
PY     := $(VENV)/bin/python
CORE   := $(VENV)/bin/neko-core

.DEFAULT_GOAL := help
.PHONY: help build shell cat claude claude-remove install uninstall api test fmt check venv clean

help: ## 列出所有指令
	@echo "make <指令>"
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

build: $(BIN) ## 編譯 Rust 執行檔（僅在原始碼變更時重新編譯）

$(BIN): Cargo.toml Cargo.lock $(wildcard src/*.rs)
	cargo build --release

shell: $(BIN) $(CORE) ## 啟動 shell，於終端機空白區域顯示貓（exit 結束）
	@$(BIN) shell

cat: $(BIN) $(CORE) ## 全螢幕模式（q 結束）
	@$(BIN)

claude: $(BIN) $(CORE) ## 將貓外掛安裝到 Claude Code（使用者範圍；直接讀取此 repo 的 mod/）
	claude plugin marketplace add "$(CURDIR)"
	claude plugin install neko@termineko
	@echo "已安裝。已開啟的 Claude Code 執行 /reload-plugins 或重新啟動後生效。"

claude-remove: ## 從 Claude Code 移除貓外掛
	-claude plugin uninstall neko@termineko
	-claude plugin marketplace remove termineko

install: $(BIN) $(CORE) ## 安裝至 ~/.local/bin（以 PREFIX=… 指定其他位置）
	install -Dm755 $(BIN) $(PREFIX)/bin/neko
	@echo "已安裝：$(PREFIX)/bin/neko"
	@echo "neko 依賴此 repo 的 $(CORE)。移動 repo 後需設定 NEKO_CORE。"

uninstall: ## 移除 make install 安裝的 neko
	rm -f $(PREFIX)/bin/neko

# 優先使用 uv，否則使用內建 venv 與 pip
$(PY):
	@echo "建立 Python 虛擬環境：$(VENV)"
	@if command -v uv >/dev/null; then uv venv -q $(VENV); else python3 -m venv $(VENV); fi

$(CORE): $(PY) pyproject.toml
	@echo "安裝 Python 核心（neko-core）至 $(VENV)。首次安裝需數分鐘。"
	@if command -v uv >/dev/null; then VIRTUAL_ENV=$(VENV) uv pip install -e '.[dev]'; else $(PY) -m pip install -e '.[dev]'; fi
	@touch $(CORE)
	@echo "Python 核心安裝完成。"

venv: $(CORE) ## 建立 Python 虛擬環境並安裝核心（neko-core、API）

api: $(CORE) ## 啟動 HTTP API（http://127.0.0.1:8000/docs）；以 PORT=… 指定埠號
	$(VENV)/bin/neko-api --port $(or $(PORT),8000)

test: $(CORE) ## 執行 Python 測試
	$(VENV)/bin/pytest -q

fmt: $(CORE) ## 格式化程式碼（Rust：rustfmt；Python：ruff）
	cargo fmt
	$(VENV)/bin/ruff format neko_api tests
	$(VENV)/bin/ruff check --fix neko_api tests

check: $(BIN) $(CORE) ## 執行全部檢查：格式、lint、測試、Claude Code 外掛
	cargo fmt --check
	cargo clippy --release -q -- -D warnings
	$(VENV)/bin/ruff format --check neko_api tests
	$(VENV)/bin/ruff check neko_api tests
	$(VENV)/bin/pytest -q
	claude plugin validate mod

clean: ## 刪除編譯產物與 Python 快取（保留 .venv）
	cargo clean
	rm -rf .pytest_cache .ruff_cache neko_api/__pycache__ tests/__pycache__
