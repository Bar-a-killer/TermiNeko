# neko 🐈 常用指令：打 make 看說明

BIN    := target/release/neko
PREFIX ?= $(HOME)/.local
VENV   := .venv
PY     := $(VENV)/bin/python

.DEFAULT_GOAL := help
.PHONY: help build shell cat room line install uninstall api test check venv clean

help: ## 列出所有指令
	@echo "make <指令>"
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: $(BIN) ## 編譯 Rust 版（有改才會重編）

$(BIN): Cargo.toml Cargo.lock $(wildcard src/*.rs)
	cargo build --release

shell: $(BIN) ## 開一個 shell，貓住在終端機的空白處（exit 離開）
	@$(BIN) shell

cat: $(BIN) ## 全螢幕的貓（q 離開）
	@$(BIN)

room: $(BIN) ## 有熱量和體重的房間版
	@$(BIN) room

line: $(BIN) ## 印一行貓的狀態
	@$(BIN) line

install: $(BIN) ## 裝到 ~/.local/bin，之後直接打 neko（PREFIX=… 可換位置）
	install -Dm755 $(BIN) $(PREFIX)/bin/neko
	@echo "裝好了：$(PREFIX)/bin/neko（試試 neko shell）"

uninstall: ## 移除 make install 裝的 neko
	rm -f $(PREFIX)/bin/neko

$(PY):
	python3 -m venv $(VENV)
	$(PY) -m pip install -q -e '.[dev]'

venv: $(PY) ## 建立 Python 虛擬環境並安裝 API

api: $(PY) ## 啟動 HTTP API（http://127.0.0.1:8000/docs）；PORT=9000 換埠
	$(VENV)/bin/neko-api --port $(or $(PORT),8000)

test: $(PY) ## 跑 Python 測試
	$(VENV)/bin/pytest -q

check: $(BIN) ## 檢查 Rust（clippy）和 Claude Code 外掛
	cargo clippy --release -q
	claude plugin validate mod

clean: ## 刪掉編譯產物
	cargo clean
