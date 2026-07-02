SHELL := /bin/bash

.DEFAULT_GOAL := help

ROOT_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))

.PHONY: help install dev dev-console dev-server dev-launcher start-local build build-console \
	build-server build-launcher package-local package-installers package-apt package-yum lint typecheck test cargo-check cargo-test check \
	preflight preflight-format preflight-app preflight-docs format-check check-compat

help: ## 显示帮助信息
	@echo 'MediaGen 根目录命令入口'
	@echo ''
	@echo '常用示例:'
	@echo '  make start-local        启动本机服务并打开 Web Console'
	@echo '  make dev                启动 Web Console 开发'
	@echo '  make check              运行常用检查'
	@echo '  make preflight          提交前完整门禁'
	@echo ''
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-24s\033[0m %s\n", $$1, $$2}'

# ─── Install ──────────────────────────────────────────────────────

install: ## 安装依赖
	cd $(ROOT_DIR) && npm ci

# ─── App ──────────────────────────────────────────────────────────

dev: ## 启动 Web Console 开发环境
	cd $(ROOT_DIR) && npm run dev:console

dev-console: ## 启动 Web Console 开发环境
	cd $(ROOT_DIR) && npm run dev:console

dev-server: ## 启动 Rust 服务端
	cd $(ROOT_DIR) && npm run dev:server

dev-launcher: ## 启动本机图标启动器开发版
	cd $(ROOT_DIR) && npm run dev:launcher

start-local: ## 使用备用脚本启动本机 Web Console
	cd $(ROOT_DIR) && bash scripts/start/start-mediagen.sh

build: build-console build-server build-launcher ## 构建 Web Console、Rust 服务端和本机启动器

build-console: ## 构建 Web Console
	cd $(ROOT_DIR) && npm run build:console

build-server: ## 构建 Rust 服务端
	cd $(ROOT_DIR) && npm run build:server

build-launcher: ## 构建本机图标启动器
	cd $(ROOT_DIR) && npm run build:launcher

package-local: build-console build-server build-launcher ## 打包本机发行目录
	cd $(ROOT_DIR) && npm run package:local

package-installers: ## 打包本机和服务器安装产物
	cd $(ROOT_DIR) && npm run package:installers

package-apt: package-installers ## 生成 apt/deb 安装包
	test -f $(ROOT_DIR)/dist-local/installers/mediagen_0.1.0_amd64.deb

package-yum: package-installers ## 生成 yum/rpm 安装包
	ls $(ROOT_DIR)/dist-local/installers/*.rpm >/dev/null

lint: ## 运行 TypeScript lint、行数和导入边界检查
	cd $(ROOT_DIR) && npm run lint

typecheck: ## 运行 TypeScript 类型检查
	cd $(ROOT_DIR) && npm run type-check

test: ## 运行前端单元测试
	cd $(ROOT_DIR) && npm run test

cargo-check: ## 运行 Rust 编译检查
	cd $(ROOT_DIR) && npm run cargo-check

cargo-test: ## 运行 Rust 单元测试
	cd $(ROOT_DIR) && npm run cargo-test

check: typecheck lint test cargo-check ## 运行常用检查

# ─── Preflight 门禁 ───────────────────────────────────────────────

preflight: preflight-format preflight-app preflight-docs ## 跑一遍核心门禁
	@echo "[make] preflight 全部通过"

preflight-format: ## 运行仓库级 Prettier 格式校验
	@echo "=== Format ==="
	@cd $(ROOT_DIR) && npm ci --ignore-scripts
	@npm run format:check

preflight-app: ## 运行 App 完整门禁
	@echo "=== MediaGen App ==="
	@cd $(ROOT_DIR) && npm ci && npm run lint && npm run type-check && npm run test && npm run coverage:gate && npm run build
	@cd $(ROOT_DIR) && npm run cargo-check

preflight-docs: ## 运行文档和兼容注释检查
	@echo "=== Docs ==="
	@bash scripts/check-compat-expiry.sh

format-check: ## 仓库级 Prettier 格式校验
	node scripts/check-prettier-format.mjs --all

check-compat: ## 检查 COMPAT 注释过期
	bash scripts/check-compat-expiry.sh
