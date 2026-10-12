SHELL := /bin/bash

.DEFAULT_GOAL := help

ROOT_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))

.PHONY: help install dev build package check deploy

help: ## 显示帮助信息
	@echo 'MediaGen 根目录命令入口'
	@echo ''
	@echo '推荐使用:'
	@echo '  make dev                同时启动服务端和 Web Console'
	@echo '  make package            生成当前系统的正式安装包'
	@echo '  make check              运行提交前完整质量门禁'
	@echo ''
	@echo '说明: build 只编译，package 才会生成可安装文件；deploy 会修改远程服务器。'
	@echo ''
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-24s\033[0m %s\n", $$1, $$2}'

# ─── Install ──────────────────────────────────────────────────────

install: ## 安装依赖
	cd $(ROOT_DIR) && npm ci

# ─── App ──────────────────────────────────────────────────────────

dev: ## 同时启动 Rust 服务端和 Web Console
	@cd $(ROOT_DIR); \
		npm run dev:server & server_pid=$$!; \
		trap 'kill "$$server_pid" 2>/dev/null || true' EXIT INT TERM; \
		npm run dev:console

build: ## 编译 Console、Server 和 Launcher，不生成安装包
	cd $(ROOT_DIR) && npm run build

package: ## 生成当前操作系统的正式安装包
	cd $(ROOT_DIR) && npm run package

check: ## 运行提交前完整质量门禁
	cd $(ROOT_DIR) && npm run check

deploy: ## 构建并部署到已配置的远程服务器
	cd $(ROOT_DIR) && npm run deploy
