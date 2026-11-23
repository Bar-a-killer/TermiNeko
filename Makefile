# neko 🐈 常用指令：打 make 看說明
PREFIX ?= $(HOME)/.local
VENV   := .venv
PY     := $(VENV)/bin/python
BIN    := $(VENV)/bin/neko

.DEFAULT_GOAL := help
.PHONY: help build shell cat codex line install uninstall api test fmt check venv clean

help: ## 列出所有指令
	@echo "make <指令>"
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: $(BIN) ## 安裝 Python 版終端機程式與依賴

shell: $(BIN) ## 開一個 shell，貓住在終端機的空白處（exit 離開）
	@$(BIN) shell

cat: $(BIN) ## 全螢幕的貓（q 離開）
	@$(BIN)

codex: $(BIN) ## 開啟 Codex CLI，輸入框下方常駐像素貓（需要 tmux）
	@NEKO_BIN="$${NEKO_BIN:-$(abspath $(BIN))}" NEKO_CORE="$${NEKO_CORE:-$(abspath $(VENV)/bin/neko-core)}" sh codex/neko/bin/neko-codex run

line: $(BIN) ## 印一行貓的狀態
	@$(BIN) line

install: $(BIN) ## 裝到 ~/.local/bin，之後直接打 neko（PREFIX=… 可換位置）
	install -Dm755 $(BIN) $(PREFIX)/bin/neko
	@echo "裝好了：$(PREFIX)/bin/neko（試試 neko shell）"
	@echo "neko 使用 $(abspath $(VENV))，請保留這個環境。"

uninstall: ## 移除 make install 裝的 neko
	rm -f $(PREFIX)/bin/neko

# 有 uv 就用 uv，不然用內建的 venv + pip
$(PY):
	@if command -v uv >/dev/null; then uv venv -q $(VENV); else python3 -m venv $(VENV); fi

$(BIN): $(PY) pyproject.toml
	@if command -v uv >/dev/null; then VIRTUAL_ENV=$(VENV) uv pip install -q -e '.[dev]'; else $(PY) -m pip install -q -e '.[dev]'; fi
	@touch $(BIN)

venv: $(BIN) ## 建立 Python 虛擬環境並安裝核心（neko-core、API）

api: $(BIN) ## 啟動 HTTP API（http://127.0.0.1:8000/docs）；PORT=9000 換埠
	$(VENV)/bin/neko-api --port $(or $(PORT),8000)

test: $(BIN) ## 跑 Python 測試
	$(VENV)/bin/pytest -q

fmt: $(BIN) ## 整理格式（ruff）
	$(VENV)/bin/ruff format neko_api neko_terminal tests
	$(VENV)/bin/ruff check --fix neko_api neko_terminal tests

check: $(BIN) ## 全部檢查一次：格式、lint、測試、Claude Code 外掛
	$(VENV)/bin/ruff format --check neko_api neko_terminal tests
	$(VENV)/bin/ruff check neko_api neko_terminal tests
	$(VENV)/bin/pytest -q
	claude plugin validate mod

clean: ## 刪掉編譯產物和 Python 快取（.venv 留著）
	rm -rf .pytest_cache .ruff_cache neko_api/__pycache__ neko_terminal/__pycache__ tests/__pycache__
