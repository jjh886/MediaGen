# MediaGen Web Console 服务端化开发规格

本文档记录 MediaGen 服务端化后的项目说明、运行逻辑、项目结构、模块职责、接口、数据表、打包方式和迁移步骤。

## 1. 项目说明

MediaGen 是一个插件化的多平台生成任务调度系统。系统通过平台插件调用不同平台账号，统一提供账号管理、任务管理、OpenAI 兼容图片接口、浏览器登录授权、人工验证接管和结果输出能力。

新版项目使用一套 Web Console 作为唯一业务界面。本机用户点击 `MediaGen` 图标后，`local-launcher` 启动本机 `mediagen-server`，再自动打开浏览器访问 Console。服务器用户直接访问部署后的 Console 地址。两种使用方式都进入同一套前端页面和同一套 Rust 后端逻辑。

项目不再维护独立桌面业务页面。Windows 和 macOS 只提供带图标的启动器，启动器不处理账号、任务、插件和 API Key 业务逻辑，只负责启动本机服务并打开网页。

## 2. 整体运行逻辑

```text
用户入口
  -> 本机：点击 MediaGen 图标
  -> 服务器：浏览器访问服务地址

apps/console
  -> 展示账号、账号组、API Key、任务、插件、代理、浏览器运行时
  -> 通过 HTTP API 调用 apps/server
  -> 通过 RemoteBrowserWorkspace 显示远程浏览器画面

apps/server
  -> 处理管理 API
  -> 处理 OpenAI 兼容 API
  -> 加载 plugins/
  -> 调度任务
  -> 选择账号
  -> 注入代理、浏览器运行时、OS 画像
  -> 执行插件 flow
  -> 需要网页操作时创建 BrowserSession

crates/mediagen-browser-gateway
  -> 启动 CloakBrowser
  -> 连接 CDP
  -> 输出网页画面流
  -> 接收鼠标、键盘、拖动、粘贴输入
  -> 保存账号 profile 和 cookies
```

## 3. 本机启动逻辑

```text
用户点击 MediaGen 图标
  -> apps/local-launcher 启动
  -> 检查 bin/mediagen-server 是否存在
  -> 检查 18080 端口
  -> 创建 data/、logs/、profiles/
  -> 启动 bin/mediagen-server
  -> 轮询 http://127.0.0.1:18080/health
  -> 健康检查通过后打开默认浏览器
  -> 用户进入 apps/console
```

启动失败处理：

```text
写入 logs/startup.log
显示小白可理解的错误信息
保留 start-mediagen.bat / start-mediagen.ps1 / start-mediagen.command / start-mediagen.sh 作为排障入口
```

## 4. 服务器部署逻辑

```text
安装 mediagen-server
  -> 读取 /etc/mediagen/config.toml
  -> 连接 PostgreSQL
  -> 加载 /usr/share/mediagen/console
  -> 加载 plugins/
  -> 启动 HTTP API
  -> 启动任务 worker
  -> 启动 BrowserGatewayService
  -> 管理员访问 Console
```

服务器部署后通过 API Key 对外提供 OpenAI 兼容接口。API Key 不直接绑定平台账号，API Key 绑定账号组，调度器从授权账号组中选择可用账号执行任务。

## 5. 账号授权逻辑

```text
管理员点击添加账号
  -> Console 创建 BrowserSession，purpose=add_account
  -> server 创建 pending account
  -> BrowserGatewayService 启动 CloakBrowser
  -> RemoteBrowserWorkspace 渲染网页内容
  -> 用户在网页区域完成登录
  -> BrowserGatewayService 保存 profile/cookies
  -> server 激活账号
  -> 账号加入账号组
```

重新登录：

```text
管理员点击重新登录
  -> Console 创建 BrowserSession，purpose=relogin_account
  -> server 锁定原账号 profile
  -> BrowserGatewayService 使用原 profile、代理、浏览器运行时
  -> 用户完成登录或验证
  -> server 更新 login_state
  -> server 释放 profile 锁
```

任务中人机验证：

```text
任务执行中触发 verification_required
  -> worker 暂停当前 attempt
  -> server 创建 BrowserSession，purpose=human_verification
  -> Console 显示验证队列
  -> 管理员进入 RemoteBrowserWorkspace
  -> 管理员完成网页验证
  -> server 标记验证完成
  -> worker 从当前步骤继续或重试当前步骤
```

## 6. 任务执行逻辑

```text
OpenAI API 请求进入 /v1/images/*
  -> server 校验 API Key
  -> 解析 API Key 授权账号组
  -> 创建 task
  -> worker 获取 task lease
  -> 调度器选择平台账号
  -> 注入 account_id、profile_dir、proxy_url、browser_runtime、os_profile
  -> plugin-runtime 执行插件 flow
  -> engine 执行通用原子能力
  -> 任务完成后写入结果和 task_events
```

任务状态：

```text
pending
processing
verification_required
verification_timeout
completed
failed
cancelled
```

## 7. 权限和隔离逻辑

```text
管理员登录 Console
  -> 管理账号、账号组、API Key、代理、插件和任务

API 调用方
  -> 只持有 API Key
  -> 不登录 Console
  -> 不直接看到平台账号、cookies、proxy_password、profile_dir

API Key
  -> 绑定一个或多个账号组
  -> 配置并发、RPM、日限额、过期时间
  -> 调度范围由 api_key_group_grants 决定
```

数据隔离：

```text
账号 profile 按 account_id 隔离
代理按 proxy_id 绑定账号
HTTP 连接池按 account_id + proxy_id 隔离
BrowserSession 同一时间只允许一个操作者 claim
敏感字段加密保存
API Key 只保存 hash
```

## 8. 当前项目

当前目录：

```text
MediaGen/
  package.json
  package-lock.json
  Makefile
  src/
  src-tauri/
  plugins/
  docs/
  scripts/
  demo/headless-cdp-login/
```

当前模块迁移表：

```text
src/                                  -> apps/console/src/
src/shared/ui/                        -> packages/ui/src/
src/features/accounts/                -> apps/console/src/features/accounts/
src/features/tasks/                   -> apps/console/src/features/tasks/
src/features/settings/                -> apps/console/src/features/settings/

src-tauri/src/types.rs                -> crates/mediagen-core/src/
src-tauri/src/plugin_loader.rs        -> crates/mediagen-plugin-runtime/src/
src-tauri/src/orchestrator.rs         -> crates/mediagen-plugin-runtime/src/
src-tauri/src/plugin_runtime.rs       -> crates/mediagen-plugin-runtime/src/
src-tauri/src/engine/                 -> crates/mediagen-engine/src/
src-tauri/src/engine/browser_engine/  -> crates/mediagen-browser-engine/src/
src-tauri/src/api_server/             -> crates/mediagen-openai-api/src/
src-tauri/src/storage/                -> crates/mediagen-storage/src/

plugins/                              -> plugins/
demo/headless-cdp-login/              -> crates/mediagen-browser-gateway 的实现参考
```

## 9. 目标目录

```text
MediaGen/
  package.json
  package-lock.json
  Cargo.toml
  Makefile
  tsconfig.base.json
  .prettierrc
  AGENTS.md

  apps/
    console/
    server/
    local-launcher/

  crates/
    mediagen-core/
    mediagen-contracts/
    mediagen-plugin-runtime/
    mediagen-engine/
    mediagen-browser-engine/
    mediagen-browser-gateway/
    mediagen-storage/
    mediagen-openai-api/

  packages/
    contracts/
    ui/
    feature-kits/

  plugins/
    plugins.index.json
    _shared/
    chatgpt/
    codex/
    doubao/
    flow/
    jimeng/

  deploy/
    local/
    systemd/
    apt/
    yum/
    docker/
    nginx/
    caddy/

  scripts/
    start/
    check-workspace-boundaries.mjs
    check-import-boundaries.mjs
    check-rust-platform-boundaries.mjs
    generate-contracts.mjs
    package-local-launcher.mjs
    package-local-release.mjs

  docs/
    mediagen-standard-monorepo-plan.md
    plugin-runtime-contract.md
    api-contract.md
    deployment-guide.md
```

## 10. 技术栈

```text
前端：React + TypeScript + Vite + Vitest
后端：Rust + Actix Web + Tokio
存储：SQLite + PostgreSQL
插件运行：Rust + rquickjs
浏览器：CloakBrowser + CDP
远程画面：CDP screencast + WebSocket/SSE + Canvas
本机入口：Rust local-launcher
脚本：Node + Shell + PowerShell
```

## 11. 运行方式

本机启动：

```text
入口：MediaGen 图标
后端：本机 mediagen-server
数据库：SQLite
地址：http://127.0.0.1:18080
界面：apps/console
浏览器会话：本机 BrowserGatewayService
```

服务器部署：

```text
入口：浏览器访问服务器地址
后端：mediagen-server
数据库：PostgreSQL
界面：apps/console
浏览器会话：服务器 BrowserGatewayService
安装：apt / yum / systemd / docker
```

worker：

```text
入口：mediagen-worker
用途：执行任务
界面：无
队列：PostgreSQL lease 或 Redis
```

## 12. 根配置

`package.json`：

```json
{
  "name": "mediagen",
  "private": true,
  "workspaces": ["apps/console", "packages/contracts", "packages/ui", "packages/feature-kits"],
  "scripts": {
    "dev:console": "npm --workspace @mediagen/console run dev",
    "dev:server": "cargo run -p mediagen-server",
    "dev:launcher": "cargo run -p mediagen-local-launcher",
    "build": "npm run build:console && npm run build:server && npm run build:launcher",
    "build:console": "npm --workspace @mediagen/console run build",
    "build:server": "cargo build -p mediagen-server --release",
    "build:launcher": "cargo build -p mediagen-local-launcher --release",
    "package:local": "node scripts/package-local-release.mjs",
    "type-check": "npm run type-check --workspaces",
    "test": "npm run test --workspaces",
    "lint": "node scripts/check-workspace-boundaries.mjs && npm run lint --workspaces",
    "format:check": "prettier --check .",
    "cargo-check": "cargo check --workspace",
    "cargo-test": "cargo test --workspace",
    "check": "npm run type-check && npm run lint && npm run test && npm run cargo-check"
  }
}
```

`Cargo.toml`：

```toml
[workspace]
resolver = "2"
members = [
  "apps/server",
  "apps/local-launcher",
  "crates/mediagen-core",
  "crates/mediagen-contracts",
  "crates/mediagen-plugin-runtime",
  "crates/mediagen-engine",
  "crates/mediagen-browser-engine",
  "crates/mediagen-browser-gateway",
  "crates/mediagen-storage",
  "crates/mediagen-openai-api",
]
```

## 13. `apps/console`

目录：

```text
apps/console/
  package.json
  index.html
  vite.config.ts
  tsconfig.json
  src/
    main.tsx
    app/
      ConsoleApp.tsx
      routes.tsx
      layout/
      startup/
    features/
      dashboard/
      accounts/
      account-groups/
      api-keys/
      remote-browser/
      verifications/
      proxies/
      browser-runtimes/
      plugins/
      tasks/
      settings/
      logs/
      setup/
    services/
      consoleApiClient.ts
      accountService.ts
      accountGroupService.ts
      apiKeyService.ts
      taskService.ts
      settingsService.ts
      browserSessionService.ts
      verificationService.ts
      runtimeMode.ts
    styles/
      globals.css
```

页面：

```text
/setup
/login
/dashboard
/accounts
/account-groups
/api-keys
/tasks
/proxies
/browser-runtimes
/plugins
/models
/logs
/settings
/browser-sessions/:id
/verifications/:id
```

页面规则：

```text
只维护一套 UI。
所有数据通过 HTTP API 获取。
不直接调用 Rust、Actix、数据库或本机脚本。
本机启动时隐藏 API Key 管理入口，除非开启本机 API Key 保护。
服务器部署时显示账号分组、API Key、代理、浏览器运行时、任务和审计日志。
账号添加、重新登录、人机验证、任务接管都进入 RemoteBrowserWorkspace。
```

## 14. `apps/server`

目录：

```text
apps/server/
  Cargo.toml
  src/
    main.rs
    config.rs
    app_state.rs
    router.rs
    auth/
      session.rs
      api_key.rs
    routes/
      health.rs
      setup.rs
      auth.rs
      api_keys.rs
      account_groups.rs
      accounts.rs
      proxies.rs
      browser_runtimes.rs
      browser_sessions.rs
      verifications.rs
      plugins.rs
      models.rs
      tasks.rs
      logs.rs
      settings.rs
      openai.rs
    services/
      admin_auth_service.rs
      api_key_service.rs
      account_service.rs
      account_group_service.rs
      proxy_service.rs
      browser_runtime_service.rs
      browser_session_service.rs
      verification_service.rs
      task_service.rs
      audit_service.rs
    workers/
      task_worker.rs
      lease_reaper.rs
      browser_session_reaper.rs
      account_health_worker.rs
      verification_reaper.rs
```

服务职责：

```text
托管 apps/console 构建产物。
提供管理 API。
提供 OpenAI 兼容 API。
加载 plugins/。
执行插件任务。
管理账号、账号组、API Key、代理、浏览器运行时。
管理 BrowserSession。
内嵌 BrowserGatewayService。
写审计日志。
```

## 15. `apps/local-launcher`

目录：

```text
apps/local-launcher/
  Cargo.toml
  build.rs
  assets/
    mediagen.ico
    mediagen.icns
    mediagen.png
  src/
    main.rs
    launch.rs
    health.rs
    open_browser.rs
    platform.rs
```

启动器行为：

```text
Windows 输出 MediaGen.exe。
macOS 输出 MediaGen.app。
点击图标后启动 bin/mediagen-server。
启动前检查端口 18080。
启动后轮询 http://127.0.0.1:18080/health。
健康检查通过后打开系统默认浏览器。
启动失败写 logs/startup.log。
启动失败时显示清楚错误。
不包含账号、任务、插件、API Key 页面或业务规则。
```

## 16. crates

`mediagen-core`：

```text
ids.rs
error.rs
mode.rs
auth.rs
api_key.rs
account.rs
account_group.rs
proxy.rs
browser_runtime.rs
browser_session.rs
verification.rs
task.rs
plugin_manifest.rs
model.rs
failure.rs
audit.rs
```

`mediagen-contracts`：

```text
openai/
admin/
events/
```

`mediagen-plugin-runtime`：

```text
loader.rs
plugin_index.rs
orchestrator.rs
template.rs
js_runtime.rs
context.rs
```

`mediagen-engine`：

```text
registry.rs
atom_executor.rs
account_engine.rs
file_engine.rs
http_engine.rs
image_engine.rs
process_engine.rs
retry.rs
condition.rs
```

`mediagen-browser-engine`：

```text
open.rs
navigation.rs
interaction.rs
file_input.rs
cookie.rs
close.rs
session.rs
process.rs
```

`mediagen-browser-gateway`：

```text
gateway.rs
session_manager.rs
cloak_runtime.rs
cdp_client.rs
screencast.rs
input_dispatch.rs
clipboard.rs
passkey_fallback.rs
proxy_auth.rs
types.rs
```

`mediagen-storage`：

```text
db.rs
migrations.rs
sqlite.rs
postgres.rs
repositories/
migrations/
```

`mediagen-openai-api`：

```text
image_request.rs
image_response.rs
image_refs.rs
image_output.rs
image_sse.rs
model_routes.rs
handlers.rs
```

## 17. packages

`packages/contracts`：

```text
src/generated/admin.ts
src/generated/openai.ts
src/generated/events.ts
src/schemas/admin.schema.json
src/schemas/openai.schema.json
```

`packages/ui`：

```text
KtButton
KtInput
KtSelect
KtModal
KtTable
KtTabs
KtBadge
KtTooltip
```

`packages/feature-kits`：

```text
accounts/AccountStatusBadge.tsx
accounts/AccountUsageBar.tsx
accounts/AccountTableBase.tsx
tasks/TaskStatusBadge.tsx
tasks/TaskTableBase.tsx
plugins/PluginStatusBadge.tsx
```

## 18. BrowserSession

用途：

```text
add_account
relogin_account
human_verification
task_takeover
debug_preview
```

状态：

```text
pending
starting
active
completed
cancelled
expired
failed
```

字段：

```text
id
purpose
status
api_key_id
operator_label
claimed_by_label
platform
account_id
pending_account_id
task_id
attempt_id
profile_dir
profile_lock_id
proxy_id
browser_runtime_id
os_profile_id
gateway_instance_id
cdp_target_id
current_url
current_title
last_screenshot_at
verification_reason
expires_at
created_at
completed_at
```

API：

```text
POST   /api/browser-sessions
GET    /api/browser-sessions/{id}
POST   /api/browser-sessions/{id}/claim
POST   /api/browser-sessions/{id}/cancel
POST   /api/browser-sessions/{id}/complete
POST   /api/browser-sessions/{id}/resize
POST   /api/browser-sessions/{id}/input
POST   /api/browser-sessions/{id}/clipboard
GET    /api/browser-sessions/{id}/events
GET    /api/browser-sessions/{id}/screencast
```

## 19. API

管理 API：

```text
GET    /health
POST   /api/setup
POST   /api/auth/login
POST   /api/auth/logout
GET    /api/settings
PUT    /api/settings
GET    /api/api-keys
POST   /api/api-keys
PUT    /api/api-keys/{id}
DELETE /api/api-keys/{id}
GET    /api/account-groups
POST   /api/account-groups
PUT    /api/account-groups/{id}
DELETE /api/account-groups/{id}
GET    /api/accounts
POST   /api/accounts
PUT    /api/accounts/{id}
DELETE /api/accounts/{id}
GET    /api/proxies
POST   /api/proxies
GET    /api/browser-runtimes
POST   /api/browser-runtimes
GET    /api/tasks
GET    /api/tasks/{id}
POST   /api/tasks/{id}/cancel
GET    /api/logs/audit
```

OpenAI 兼容 API：

```text
GET  /v1/models
POST /v1/images/generations
POST /v1/images/edits
```

## 20. 数据表

```text
api_keys
account_groups
account_group_members
api_key_group_grants
platform_accounts
proxies
browser_profiles
browser_runtimes
os_profiles
browser_sessions
verification_sessions
tasks
task_attempts
task_events
account_leases
audit_logs
settings
plugin_installations
model_routes
```

加密字段：

```text
cookies
api_token
refresh_token
proxy_password
```

只保存哈希：

```text
api_key_hash
admin_password_hash
```

## 21. 账号和 API Key

关系：

```text
API Key -> Account Groups -> Platform Accounts
```

第一阶段：

```text
单管理员
多个 API Key
多个账号组
多个平台账号
API Key 绑定一个或多个账号组
调度器只使用 API Key 授权账号组内的账号
```

暂不实现：

```text
普通用户门户
用户组
租户
计费
订阅
支付
```

## 22. 代理和浏览器运行时

代理：

```text
支持 socks5
支持 socks5h
账号可绑定 proxy_id
浏览器登录、浏览器任务、HTTP 请求使用同一个账号代理
HTTP 连接池按 account_id + proxy_id 隔离
```

浏览器运行时：

```text
browser_runtimes 表保存可选项
用户只从下拉框选择
不让用户手写浏览器路径
默认运行时为 CloakBrowser
```

OS 画像：

```text
os_profiles 表保存可选项
支持 Windows 画像
支持 macOS 画像
用户只从下拉框选择
不修改服务器真实系统
```

## 23. 插件

目录保持：

```text
plugins/
  plugins.index.json
  _shared/
  chatgpt/
  codex/
  doubao/
  flow/
  jimeng/
```

插件运行上下文：

```text
account_id
api_key_id
allowed_group_ids
profile_dir
account_cookies
proxy_url
browser_runtime
os_profile
workspace_dir
browser_session_id
chrome_path
```

规则：

```text
平台规则只放插件。
Rust core 不写平台规则。
新增原子能力使用通用 action 名称。
插件 manifest 不写 API Key、账号组、代理规则。
```

## 24. 打包

本机发行包：

```text
MediaGen-local/
  MediaGen.exe
  MediaGen.app
  bin/
    mediagen-server
    cloakbrowser/
  console/
  plugins/
  data/
  logs/
  icons/
    mediagen.ico
    mediagen.icns
    mediagen.png
  start-mediagen.bat
  start-mediagen.ps1
  start-mediagen.command
  start-mediagen.sh
  README.txt
```

Windows：

```text
输出 MediaGen.exe
内嵌 mediagen.ico
创建桌面快捷方式
创建开始菜单快捷方式
点击图标启动服务
启动成功后打开默认浏览器
```

macOS：

```text
输出 MediaGen.app
使用 mediagen.icns
点击 App 启动服务
启动成功后 open http://127.0.0.1:18080
```

Linux：

```text
提供 start-mediagen.sh
提供 systemd service
提供 apt 包
提供 yum 包
```

服务器路径：

```text
/usr/bin/mediagen-server
/etc/mediagen/config.toml
/var/lib/mediagen
/var/lib/mediagen/profiles
/var/lib/mediagen/workspace
/var/lib/mediagen/results
/var/log/mediagen
/usr/share/mediagen/console
```

## 25. Makefile

```makefile
install
dev-console
dev-server
dev-launcher
start-local
build-console
build-server
build-launcher
package-local
package-apt
package-yum
test
lint
typecheck
cargo-check
cargo-test
check
preflight
```

## 26. 质量检查

```text
npm run format:check
npm run type-check --workspaces
npm run lint --workspaces
npm run test --workspaces
cargo fmt --check --all
cargo clippy --workspace
cargo test --workspace
cargo check --workspace
plugin schema check
contract generation check
workspace boundary check
Rust platform boundary check
```

边界检查：

```text
packages/ui 不能依赖 app。
packages/feature-kits 不能依赖 server API。
apps/console 不能直接依赖 Actix。
apps/local-launcher 不能包含业务规则。
scripts/start 不能包含业务规则。
crates/mediagen-core 不能依赖 Actix、数据库、浏览器。
crates/mediagen-plugin-runtime 不能写平台名。
crates/mediagen-engine 不能写平台名。
```

## 27. 迁移步骤

M0：

```text
补插件运行测试。
补账号添加和重新登录测试。
补 OpenAI 图片接口 contract 测试。
保留当前应用可运行。
```

M1：

```text
新增 apps/。
新增 crates/。
新增 packages/。
新增根 Cargo.toml workspace。
修改根 package.json workspaces。
```

M2：

```text
移动 src/ 到 apps/console/src/。
迁移 src/shared/ui 到 packages/ui。
迁移账号、任务、设置页面到 apps/console/src/features/。
让 apps/console 只通过 HTTP API 获取数据。
```

M3：

```text
新增 apps/server。
实现 /health。
实现 /api/setup。
实现 /api/auth/login。
实现 /v1/models。
托管 apps/console 构建产物。
```

M4：

```text
抽 crates/mediagen-core。
抽 crates/mediagen-plugin-runtime。
抽 crates/mediagen-engine。
抽 crates/mediagen-storage。
抽 crates/mediagen-openai-api。
```

M5：

```text
实现账号管理 API。
实现账号组 API。
实现 API Key API。
实现代理 API。
实现浏览器运行时 API。
实现任务 API。
```

M6：

```text
实现 crates/mediagen-browser-gateway。
迁移 demo 的 screencast。
迁移 demo 的输入转发。
实现 RemoteBrowserWorkspace。
打通 BrowserSession API。
```

M7：

```text
添加账号走 BrowserSession。
重新登录走 BrowserSession。
人机验证走 BrowserSession。
任务接管走 BrowserSession。
移除正式流程里的系统浏览器弹窗。
```

M8：

```text
实现服务端任务调度。
实现账号租约。
实现代理注入。
实现插件运行上下文注入。
跑通 API 型插件。
跑通浏览器型插件。
```

M9：

```text
实现 apps/local-launcher。
输出 Windows MediaGen.exe。
输出 macOS MediaGen.app。
输出 Linux start-mediagen.sh。
实现 package-local-release.mjs。
```

M10：

```text
新增 systemd。
新增 apt。
新增 yum。
新增 docker compose。
新增备份恢复脚本。
新增升级迁移脚本。
```
