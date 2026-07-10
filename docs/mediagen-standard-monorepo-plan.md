# MediaGen 当前服务端化方案

MediaGen 当前只保留一套 Web Console + Rust Server + 本机启动器。Windows、macOS、Linux 都运行同一个 `mediagen-server`；本机图标只负责启动服务并打开网页；服务器直接常驻运行服务。

核心原则：

```text
平台规则放到 plugins/{platform}
通用能力放到 Rust server / crates
前端只维护 apps/console 一套界面
本机和服务器只区别部署方式，不区别业务逻辑
```

## 1. 当前目录

```text
apps/console
  React Web Console。负责账号、任务、设置、日志、远程浏览器接管。

apps/server
  Rust 服务端。负责管理 API、OpenAI 兼容 API、插件调度、账号/代理/API Key、浏览器会话、日志和静态资源托管。

apps/local-launcher
  本机启动器。负责找 server、建数据目录、启动服务、等待 /health、打开 Console、显示 macOS/Windows 托盘。

crates/mediagen-browser-gateway
  CloakBrowser/CDP 通用能力。负责会话、截图流、输入、剪贴板、历史、代理认证、原生弹窗、浏览器画像。

crates/mediagen-core
  插件 manifest、模型、等级和通用枚举。

crates/mediagen-plugin-runtime
  插件加载、索引、alias、flow step 兼容解析。

plugins
  ChatGPT、Codex、豆包、即梦、Flow 等平台插件。

deploy
  systemd、apt、yum、Docker、本机安装说明。

scripts
  启动、打包、CloakBrowser 下载、质量门禁。
```

## 2. 运行模式

本机模式：

```text
用户点击 MediaGen 图标
  -> local-launcher 启动 mediagen-server
  -> server 监听 127.0.0.1:18080
  -> 打开默认浏览器访问 Console
  -> data/logs/profiles 写到用户可写目录
```

服务器模式：

```text
systemd / Docker / 终端启动 mediagen-server
  -> server 监听 0.0.0.0:18080 或配置端口
  -> nginx / Caddy / Tunnel 暴露 Console
  -> 管理员通过浏览器管理账号、代理、API Key 和任务
```

Linux 服务器常驻运行，不提供 Web Console 退出按钮。Windows 和 macOS 本机启动器提供托盘退出。

## 3. Web Console

Web Console 本机和服务器共用一套。

页面：

```text
账号：添加账号、重新登录、账号配置、浏览器系统/版本、账号级代理、API Key 绑定。
任务：任务状态、取消任务、结果和错误。
设置：API 服务、平台模型、系统信息、Windows/macOS 退出入口。
日志：审计日志、任务事件、清理日志。
API Key：自动生成、复制、删除，作为外部调用凭证。
代理：创建、删除 SOCKS5/SOCKS5H 保存代理。
浏览器：查看 CloakBrowser runtime 安装状态。
人机接管：打开任务触发的远程浏览器接管会话。
```

前端请求必须放在 `shared/services` 或 feature service，组件不能直接绕过 service 调服务端接口。

## 4. 账号添加和重新登录

账号添加统一使用远程浏览器渲染，不弹真实系统浏览器窗口。

流程：

```text
选择平台、代理、浏览器系统和版本
  -> server 创建 BrowserSession
  -> Console 显示远程网页内容
  -> 管理员在网页里登录
  -> 插件 login_detection 检测成功
  -> Console 自动关闭窗口
  -> server 保存账号、profile、代理和浏览器画像
```

窗口要求：

```text
默认大小不超过当前浏览器可见区域
窗口可调整大小
内容区随窗口同步 resize
只显示网页内容，不显示真实浏览器地址栏
顶部只保留前进、后退、刷新图标
不提供“完成登录”按钮
检测成功后自动关闭
```

重新登录复用原账号 `profile_dir`，默认沿用原账号代理和浏览器画像，允许临时覆盖。

## 5. 代理

代理支持 SOCKS5 / SOCKS5H。

入口：

```text
管理页创建保存代理
添加账号时选择保存代理
添加账号时手动填写一次性代理
账号配置页保存账号级代理
```

创建浏览器会话时：

```text
传 proxy_id
  -> server 查保存代理
  -> 写入 proxy_server/proxy_username/proxy_password metadata
  -> 保存代理优先于前端 metadata.proxy_server

不传 proxy_id
  -> 使用 metadata.proxy_server
  -> 重新登录默认使用账号 proxy_url
```

账号保存时：

```text
保存 proxy_id 和 proxy_server
不把保存代理密码复制进账号记录
任务执行时按 proxy_id 读取保存代理
内部拼出带认证的运行时代理地址
保存代理被删除或停用后，该账号不再使用这个保存代理
```

插件 HTTP 请求默认走账号代理；插件明确设置 `use_account_proxy=false` 时不走。

## 6. API Key

API Key 是当前轻量多用户隔离入口。

规则：

```text
API Key 由系统自动生成，用户不手动输入
全局 API Key 可以调度当前平台下的可用账号
账号也可单独填写可用 API Key，用于只允许指定 Key 使用该账号
旧 settings.api_keys 会迁移为哈希化 API Key 记录
服务端不回传完整 API Key
```

后续真实用户、用户组、计费和订阅，在 API Key 之上扩展。

## 7. 浏览器通用能力

`crates/mediagen-browser-gateway` 提供：

```text
创建/取消/完成会话
导航、前进、后退、刷新
调整视口和截图流
CDP 调用
鼠标移动、点击、拖动、滚轮
键盘输入、复制、粘贴
上传、下载
原生弹窗处理
代理认证
浏览器系统和版本画像
```

Console 只能调用 server 的 `/api/browser-sessions/*`，不能直接连接 CDP。

## 8. CloakBrowser

浏览器运行时统一使用 CloakBrowser。

公开发行包规则：

```text
make package-installers
  -> 执行 package-local-release
  -> 打包 server、console、plugins、launcher
  -> 执行 ensure-cloakbrowser-runtime
  -> 按当前打包平台下载最新开源版 CloakBrowser
  -> 写入 dist-local/MediaGen-local/browser-runtime
  -> 下载或校验失败时直接打包失败
```

用户启动应用：

```text
点击 MediaGen 图标或运行 start-mediagen 脚本
  -> 启动本地 server
  -> server 从安装目录 browser-runtime 或 CLOAK_BROWSER_PATH 定位 CloakBrowser
  -> 打开 Console
  -> 添加账号、人工接管和插件执行统一通过浏览器会话原子能力使用 CloakBrowser
```

支持 macOS arm64/x64、Linux arm64/x64、Windows x64。Console 不提供浏览器检查、安装、修复页面；如果运行时缺失，具体浏览器会话返回明确错误。

单独打包 Linux 安装包：

```text
make package-apt
  -> 只生成 deb，不要求 rpmbuild

make package-yum
  -> 只生成 rpm，不要求 dpkg-deb
```

## 9. 插件边界

插件负责平台规则：

```text
登录地址
OAuth 参数
页面选择器
账号检测脚本
任务执行脚本
错误文本分类
结果提取
人机验证判断
会员和额度识别
```

server 负责通用编排：

```text
加载插件
校验 API Key
选择账号
创建任务和浏览器会话
调用通用能力
保存结果和日志
返回 OpenAI 兼容响应
```

server 不写死 ChatGPT、Codex、豆包等平台页面流程。平台差异放到 `plugins/{platform}`。

## 10. 插件兼容

现有插件必须继续可用，不要求重写。

兼容字段：

```text
manifest.json / plugins.index.json
adapter_mode / login_url / workspace_url
models / tiers
login_detection / login_check / login_oauth_pkce
flows / registration
```

flow step 同时支持旧扁平写法和新 `input` 写法。没有 `input` 时，orchestrator 把控制字段之外的字段收集成 action input。`_comment` 只用于说明，不传给 action。

当前插件：

```text
chatgpt：ChatGPT 网页插件，alias 为 gpt-image-1
codex：Codex / GPT Image 2 插件，alias 为 gpt-image-2
doubao：豆包插件
jimeng：即梦插件
flow：Flow 插件
```

新增 alias 前必须检查 `plugins/*/manifest.json` 和 alias 解析测试，避免冲突。

## 11. OpenAI 兼容 API

服务端提供：

```text
GET  /v1/models
POST /v1/images/generations
POST /v1/images/edits
POST /v1/images/variations
```

处理流程：

```text
校验 Bearer API Key
根据模型找到插件和模型定义
按 API Key 授权范围选择账号
创建任务
执行插件 flow
返回 OpenAI 兼容 JSON
stream=true 时返回 text/event-stream
```

当前 SSE：

```text
event: image_generation.completed
data: <OpenAI 兼容响应>

event: image_edit.completed
data: <OpenAI 兼容响应>

event: image_variation.completed
data: <OpenAI 兼容响应>

event: done
data: [DONE]
```

文件接口：

```text
POST /v1/files
  -> multipart/form-data: purpose + file
  -> 保存到 data/openai-files
  -> 返回 file-xxx

GET /v1/files
GET /v1/files/{file_id}
GET /v1/files/{file_id}/content
DELETE /v1/files/{file_id}

图片编辑和图片变化接口可以使用 images[].file_id 和 mask.file_id。
```

## 12. 任务和账号

任务状态：

```text
pending / queued / processing / verification_required / completed / failed / cancelled
```

账号字段：

```text
platform / name / email / avatar_url / tier / login_state
used_today / daily_limit / used_total / total_limit
concurrency_limit / task_interval_seconds
proxy_url / proxy_id / browser_os / browser_version / api_key / profile_dir
```

调度先判断 API Key 是否为全局 Key；如果不是全局 Key，则只选择账号自身授权的账号。随后再按登录状态、额度、并发、任务间隔选择账号。

## 13. 人工接管

插件遇到验证码、登录过期、二次确认时，返回标准验证需求，由服务端保留浏览器会话并让 Console 接管。

接管能力：

```text
实时网页画面
同步视口大小
鼠标移动、点击、拖动
键盘输入
复制粘贴
前进、后退、刷新
检测完成后自动关闭
```

原生弹窗、WebAuthn/passkey、Basic Auth 不靠页面关键字判断，浏览器网关按 CDP 原生事件处理。

## 14. 数据保存

当前存储由 `apps/server/src/store*.rs` 管理，文件名 `mediagen.sqlite`，内部保留 JSON snapshot 兼容逻辑。

保存内容：

```text
accounts / tasks / settings / api_keys / proxies / audit_logs / task_events
```

敏感字段不要写入日志。API Key 只存哈希和前后缀。代理密码只在启动浏览器或 HTTP 请求时进入运行时 metadata。

## 15. 打包和启动

发行目录：

```text
dist-local/MediaGen-local
  bin/mediagen-server
  MediaGen 或 MediaGen.exe
  console
  plugins
  browser-runtime
  icons
  start-mediagen.sh / start-mediagen.ps1
```

安装包：

```text
macOS：MediaGen.app、DMG、tar.gz
Windows：Setup.exe、zip、tar.gz
Linux：deb、rpm、tar.gz
```

Linux 包安装后使用 systemd 常驻，环境配置在 `/etc/mediagen/mediagen.env`。

## 16. 质量门禁

每轮改造结束必须跑：

```text
npm run check
node scripts/gates/run-start-quality-gate.mjs --modules app --include-coverage
```

`npm run check` 包含格式、类型、lint、前端测试、覆盖率、构建、cargo fmt、clippy、cargo test、cargo check、COMPAT 注释检查。

覆盖率门禁：

```text
App UI >= 75%
Rust 核心层 >= 75%
```

新增 service、hook、API、store、flow、浏览器通用能力时必须补测试。

## 17. 开发规则

```text
新增平台能力：先改 plugins/{platform}
新增浏览器能力：优先放 crates/mediagen-browser-gateway
新增管理页面：前端放 features，API 放 service，后端补 route 和测试
新增打包能力：同步检查 package-local-release、package-local-installers、deploy 和 local-launcher
```

## 18. 验收清单

```text
npm run check 通过
start-quality 通过
所有代码文件不超过 500 行
没有重复实现
插件 manifest alias 没有冲突
账号添加走远程网页渲染
代理和 API Key 是真实可用逻辑
公开包随安装包内置当前平台 CloakBrowser 二进制
打包阶段下载或校验 CloakBrowser 失败必须直接失败
Windows/macOS 有托盘退出
Linux 保持常驻服务
```
