#!/usr/bin/env bash
set -euo pipefail

# 用法：./scripts/deploy-server-release.sh [服务器] [SSH端口]
# 固定使用 root 和本机 SSH 默认密钥／agent，应用监听 18080。
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${1:-}" == '--help' || "${1:-}" == '-h' ]]; then
  echo '用法：./scripts/deploy-server-release.sh [服务器=8.148.255.185] [SSH端口=26822]'
  exit 0
fi
[[ $# -le 2 ]] || { echo '只接受服务器和 SSH 端口两个参数' >&2; exit 1; }
SERVER="${1:-8.148.255.185}"
PORT="${2:-26822}"
[[ "$SERVER" =~ ^[a-zA-Z0-9][a-zA-Z0-9.-]*$ ]] || { echo '服务器地址无效' >&2; exit 1; }
[[ "$PORT" =~ ^[0-9]{1,5}$ ]] && (( 10#$PORT >= 1 && 10#$PORT <= 65535 )) || {
  echo 'SSH 端口必须为 1–65535' >&2; exit 1;
}
for command in ssh tar; do
  command -v "$command" >/dev/null || { echo "本机缺少 $command" >&2; exit 1; }
done
SSH=(ssh -p "$PORT" -o BatchMode=yes -o PasswordAuthentication=no
  -o KbdInteractiveAuthentication=no -o ConnectTimeout=15
  -o ServerAliveInterval=10 -o ServerAliveCountMax=6
  -o StrictHostKeyChecking=accept-new "root@$SERVER")
RELEASE="$(date -u +%Y%m%d-%H%M%S)-$$-$RANDOM"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

printf '\n[部署] %s:%s，检查 SSH 密钥登录\n' "$SERVER" "$PORT"
"${SSH[@]}" 'test "$(id -u)" = 0 && command -v apt-get >/dev/null && command -v systemctl >/dev/null' || {
  echo '连接失败，或服务器不是支持 systemd 的 Debian/Ubuntu。请确认本机 SSH 密钥已授权。' >&2
  exit 1
}
printf '[部署] 打包并上传源码（包含未提交修改）\n'
COPYFILE_DISABLE=1 tar -C "$ROOT_DIR" -czf "$TEMP_DIR/source.tar.gz" \
  --exclude=node_modules --exclude=target --exclude=dist --exclude=coverage \
  --exclude=.git --exclude='.env*' --exclude='*.pem' --exclude='*.key' \
  --exclude='*.p12' --exclude='*.log' --exclude=.DS_Store \
  Cargo.toml Cargo.lock rust-toolchain.toml package.json package-lock.json apps crates plugins scripts
"${SSH[@]}" "umask 077; mkdir -p /var/cache/mediagen/incoming; cat > /var/cache/mediagen/incoming/$RELEASE.tar.gz" < "$TEMP_DIR/source.tar.gz"

"${SSH[@]}" bash -s -- "$RELEASE" <<'REMOTE'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
export PATH="/root/.cargo/bin:$PATH"
RELEASE="$1"
APP=/opt/mediagen
SRC=/opt/mediagen-src
CACHE=/var/cache/mediagen
DATA=/var/lib/mediagen
mkdir -p "$CACHE" "$APP" "$SRC"
exec 9>"$CACHE/deploy.lock"
flock -n 9 || { echo '已有部署正在运行，请稍后重试' >&2; exit 1; }
STAGING="$(mktemp -d "$CACHE/incoming/source.XXXXXX")"
trap 'rm -rf "$STAGING"; rm -f "$CACHE/incoming/$RELEASE.tar.gz"' EXIT

printf '[部署] 自动安装缺少的系统依赖\n'
if ! apt-cache show libnss3 >/dev/null 2>&1; then
  apt-get update -qq
fi
packages=(ca-certificates curl tar gzip unzip python3 rsync nodejs npm
  build-essential pkg-config libssl-dev cmake clang libclang-dev perl
  ffmpeg libnss3 libx11-6 libxcomposite1 libxdamage1 libxrandr2 libgbm1
  libdrm2 fonts-noto-cjk fonts-wqy-microhei)
# Debian 13 / Ubuntu 24 使用 t64 包名，兼容仍提供旧包名的发行版。
for lib in libgtk-3-0 libasound2 libatk-bridge2.0-0 libcups2; do
  if apt-cache show "${lib}t64" >/dev/null 2>&1; then
    packages+=("${lib}t64")
  else
    packages+=("$lib")
  fi
done
missing=()
for package in "${packages[@]}"; do
  [[ "$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || true)" == 'install ok installed' ]] || missing+=("$package")
done
if (( ${#missing[@]} )); then
  apt-get update -qq
  apt-get install -y --no-install-recommends "${missing[@]}"
fi
node -e 'if (Number(process.versions.node.split(".")[0]) < 20) process.exit(1)' || {
  echo '需要 Node.js 20 或更新版本，请使用 Debian 13 / Ubuntu 24 或更新系统' >&2; exit 1;
}
if ! command -v rustc >/dev/null || ! rustc --version | python3 -c 'import sys; v=tuple(map(int,sys.stdin.read().split()[1].split(".")[:2])); sys.exit(v < (1,98))'; then
  if command -v rustup >/dev/null; then
    rustup toolchain install stable --profile minimal
    rustup default stable
  else
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 https://sh.rustup.rs -o "$STAGING/rustup-init.sh"
    sh "$STAGING/rustup-init.sh" -y --profile minimal --default-toolchain stable --no-modify-path
  fi
fi

printf '[部署] 同步源码，复用 npm / Cargo 缓存\n'
tar -xzf "$CACHE/incoming/$RELEASE.tar.gz" -C "$STAGING"
rsync -a --delete --exclude=node_modules --exclude=target --exclude=dist \
  --exclude='.env*' --exclude=.deploy-npm-hash --exclude=rustup-init.sh "$STAGING/" "$SRC/"
cd "$SRC"
NPM_HASH="$(sha256sum package.json package-lock.json apps/console/package.json | sha256sum | cut -d' ' -f1)"
if [[ ! -d node_modules || ! -x node_modules/.bin/vite || "$(cat .deploy-npm-hash 2>/dev/null || true)" != "$NPM_HASH" ]]; then
  npm ci --ignore-scripts --no-audit --no-fund
  printf '%s\n' "$NPM_HASH" > .deploy-npm-hash
fi
npm run build:console
export CARGO_TARGET_DIR="$CACHE/cargo-target"
export CARGO_BUILD_JOBS="$(python3 - <<'PY'
import os
from pathlib import Path
memory = dict(line.split(':', 1) for line in Path('/proc/meminfo').read_text().splitlines())
print(max(1, min(os.cpu_count() or 1, int(memory['MemAvailable'].split()[0]) // 1800000, 8)))
PY
)"
export CMAKE_BUILD_PARALLEL_LEVEL="$CARGO_BUILD_JOBS"
cargo build --locked --release -p mediagen-server

if ! id mediagen >/dev/null 2>&1; then
  useradd --system --home "$DATA" --shell /usr/sbin/nologin mediagen
fi
install -d -o mediagen -g mediagen "$DATA" "$DATA/profiles" /var/log/mediagen
if [[ ! -x "$APP/browser-runtime/chrome" ]]; then
  node scripts/ensure-cloakbrowser-runtime.mjs --output "$APP/browser-runtime" --cache "$CACHE/cloakbrowser"
fi
ldd "$APP/browser-runtime/chrome" | awk '/not found/ { print; missing=1 } END { exit missing }'
# 首次安装才创建配置；后续部署绝不重置账号、密码、代理或证书。
if [[ ! -f /etc/mediagen/mediagen.env ]]; then
  install -d -m 700 /etc/mediagen
  python3 - <<'PY'
import hashlib, os, secrets
from pathlib import Path
os.umask(0o077)
config = '''MEDIAGEN_HOST=0.0.0.0
MEDIAGEN_PORT=18080
MEDIAGEN_CONSOLE_DIR=/opt/mediagen/console
MEDIAGEN_PLUGINS_DIR=/opt/mediagen/plugins
MEDIAGEN_DATA_DIR=/var/lib/mediagen
MEDIAGEN_LOGS_DIR=/var/log/mediagen
MEDIAGEN_PROFILES_DIR=/var/lib/mediagen/profiles
CLOAK_BROWSER_BUNDLE_DIR=/opt/mediagen/browser-runtime
'''
if not Path('/var/lib/mediagen/admin.sqlite').exists():
    password = secrets.token_urlsafe(24)
    Path('/etc/mediagen/initial-admin-password').write_text(password + '\n')
    config += 'MEDIAGEN_ADMIN_USERNAME=admin\nMEDIAGEN_ADMIN_PASSWORD_HASH=' + hashlib.sha256(password.encode()).hexdigest() + '\n'
    print('首次安装：admin 初始密码保存在服务器 /etc/mediagen/initial-admin-password（仅 root 可读）')
Path('/etc/mediagen/mediagen.env').write_text(config)
PY
fi
if [[ ! -f /etc/systemd/system/mediagen-server.service ]]; then
  cat > /etc/systemd/system/mediagen-server.service <<'UNIT'
[Unit]
Description=MediaGen Server
After=network-online.target
Wants=network-online.target
[Service]
Type=simple
User=mediagen
Group=mediagen
WorkingDirectory=/opt/mediagen
EnvironmentFile=/etc/mediagen/mediagen.env
ExecStart=/opt/mediagen/bin/mediagen-server
Restart=always
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
[Install]
WantedBy=multi-user.target
UNIT
fi
systemctl daemon-reload
systemctl enable mediagen-server
printf '[部署] 构建完成，备份并切换；未完成的持久队列将在重启后恢复\n'
python3 - "$RELEASE" <<'INSTALL'
import hashlib, json, os, pathlib, pwd, shutil, sqlite3, subprocess, sys, time, urllib.request
app = pathlib.Path('/opt/mediagen')
source = pathlib.Path('/opt/mediagen-src')
data = pathlib.Path('/var/lib/mediagen')
release = app / 'releases' / sys.argv[1]
release.mkdir(parents=True, mode=0o700)
staged = release / 'staged'
previous = release / 'previous'
(staged / 'bin').mkdir(parents=True)
previous.mkdir()
shutil.copy2('/var/cache/mediagen/cargo-target/release/mediagen-server', staged / 'bin/mediagen-server')
for name, origin in [('console', source / 'apps/console/dist'), ('plugins', source / 'plugins')]:
    shutil.copytree(origin, staged / name)
owner = pwd.getpwnam('mediagen')
for root, dirs, files in os.walk(staged):
    os.chown(root, owner.pw_uid, owner.pw_gid)
    for name in files:
        os.chown(pathlib.Path(root) / name, owner.pw_uid, owner.pw_gid)
expected_html = hashlib.sha256((staged / 'console/index.html').read_bytes()).hexdigest()
was_running = subprocess.run(['systemctl', 'is-active', '--quiet', 'mediagen-server']).returncode == 0
swapped = []
health_client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
try:
    subprocess.run(['systemctl', 'stop', 'mediagen-server'], check=True)
    for name in ('mediagen.sqlite', 'admin.sqlite'):
        db = data / name
        if db.exists():
            with sqlite3.connect(f'file:{db}?mode=ro', uri=True) as origin, sqlite3.connect(release / name) as backup:
                origin.backup(backup)
            (release / name).chmod(0o600)
    for name in ('bin', 'console', 'plugins'):
        target = app / name
        if target.exists():
            target.rename(previous / name)
        swapped.append(name)
        (staged / name).rename(target)
    subprocess.run(['systemctl', 'start', 'mediagen-server'], check=True)
    for attempt in range(30):
        try:
            with health_client.open('http://127.0.0.1:18080/health', timeout=2) as response:
                health = json.load(response)
            with health_client.open('http://127.0.0.1:18080/', timeout=2) as response:
                html = response.read()
            if health.get('ok') and hashlib.sha256(html).hexdigest() == expected_html:
                break
        except (OSError, ValueError):
            pass
        time.sleep(1)
    else:
        raise RuntimeError('健康检查或前端版本核验失败')
except BaseException:
    subprocess.run(['systemctl', 'stop', 'mediagen-server'], check=False)
    for name in reversed(swapped):
        target = app / name
        if target.exists():
            target.rename(release / ('failed-' + name))
        if (previous / name).exists():
            (previous / name).rename(target)
    if was_running:
        subprocess.run(['systemctl', 'start', 'mediagen-server'], check=True)
    print(f'发布失败，已恢复旧程序；数据库备份：{release}', file=sys.stderr)
    raise
print(f'部署成功，进程 {health["pid"]}；旧版本和数据库备份：{release}')
INSTALL
REMOTE
printf '\n部署完成：http://%s:18080（已有域名和 HTTPS 配置保持不变）\n' "$SERVER"
