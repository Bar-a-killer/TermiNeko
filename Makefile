# neko 🐈 常用指令：打 make 看說明
# Rust（src/）只負責畫面和終端機互動；營養、心情、紀錄都在 Python 核心（neko_api/，neko-core 指令）。
# 所以跑貓之前要先有 .venv（第一次會自動建）。

BIN    := target/release/neko
PREFIX ?= $(HOME)/.local
VENV   := .venv
PY     := $(VENV)/bin/python
CORE   := $(VENV)/bin/neko-core

.DEFAULT_GOAL := help
.PHONY: help build shell cat line install uninstall api test fmt check venv clean

help: ## 列出所有指令
	@echo "make <指令>"
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: $(BIN) ## 編譯 Rust 版（有改才會重編）

$(BIN): Cargo.toml Cargo.lock $(wildcard src/*.rs)
	cargo build --release

shell: $(BIN) $(CORE) ## 開一個 shell，貓住在終端機的空白處（exit 離開）
	@$(BIN) shell

cat: $(BIN) $(CORE) ## 全螢幕的貓（q 離開）
	@$(BIN)

line: $(BIN) $(CORE) ## 印一行貓的狀態
	@$(BIN) line

install: $(BIN) $(CORE) ## 裝到 ~/.local/bin，之後直接打 neko（PREFIX=… 可換位置）
	install -Dm755 $(BIN) $(PREFIX)/bin/neko
	@echo "裝好了：$(PREFIX)/bin/neko（試試 neko shell）"
	@echo "neko 會用這個 repo 的 $(CORE) 算營養，repo 不要搬走（或設定 NEKO_CORE）"

uninstall: ## 移除 make install 裝的 neko
	rm -f $(PREFIX)/bin/neko

# 有 uv 就用 uv，不然用內建的 venv + pip
$(PY):
	@if command -v uv >/dev/null; then uv venv -q $(VENV); else python3 -m venv $(VENV); fi

$(CORE): $(PY) pyproject.toml
	@if command -v uv >/dev/null; then VIRTUAL_ENV=$(VENV) uv pip install -q -e '.[dev]'; else $(PY) -m pip install -q -e '.[dev]'; fi
	@touch $(CORE)

venv: $(CORE) ## 建立 Python 虛擬環境並安裝核心（neko-core、API）

api: $(CORE) ## 啟動 HTTP API（http://127.0.0.1:8000/docs）；PORT=9000 換埠
	$(VENV)/bin/neko-api --port $(or $(PORT),8000)

test: $(CORE) ## 跑 Python 測試
	$(VENV)/bin/pytest -q

fmt: $(CORE) ## 整理格式（Rust：rustfmt；Python：ruff）
	cargo fmt
	$(VENV)/bin/ruff format neko_api tests
	$(VENV)/bin/ruff check --fix neko_api tests

check: $(BIN) $(CORE) ## 全部檢查一次：格式、lint、測試、Claude Code 外掛
	cargo fmt --check
	cargo clippy --release -q -- -D warnings
	$(VENV)/bin/ruff format --check neko_api tests
	$(VENV)/bin/ruff check neko_api tests
	$(VENV)/bin/pytest -q
	claude plugin validate mod

clean: ## 刪掉編譯產物和 Python 快取（.venv 留著）
	cargo clean
	rm -rf .pytest_cache .ruff_cache neko_api/__pycache__ tests/__pycache__
