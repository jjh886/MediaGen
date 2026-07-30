#!/usr/bin/env bash
set -euo pipefail

# MediaGen 服务端一键部署脚本。
# 默认适配当前服务器：47.107.252.121:30222 / media-gen.realove.chat。
# 私密配置可以放到仓库根目录 .env.deploy.local，文件已被 .gitignore 覆盖。

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCAL_ENV_FILE="${ROOT_DIR}/.env.deploy.local"

if [[ -f "${LOCAL_ENV_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "${LOCAL_ENV_FILE}"
  set +a
fi

DEPLOY_HOST="${MEDIAGEN_DEPLOY_HOST:-47.107.252.121}"
DEPLOY_PORT="${MEDIAGEN_DEPLOY_PORT:-30222}"
DEPLOY_USER="${MEDIAGEN_DEPLOY_USER:-root}"
DEPLOY_DOMAIN="${MEDIAGEN_DEPLOY_DOMAIN:-media-gen.realove.chat}"
REMOTE_SRC="${MEDIAGEN_REMOTE_SRC:-/opt/mediagen-src}"
REMOTE_APP="${MEDIAGEN_REMOTE_APP:-/opt/mediagen}"
REMOTE_DATA="${MEDIAGEN_REMOTE_DATA:-/var/lib/mediagen}"
REMOTE_LOGS="${MEDIAGEN_REMOTE_LOGS:-/var/log/mediagen}"
REMOTE_ENV_FILE="${MEDIAGEN_REMOTE_ENV_FILE:-/etc/mediagen/mediagen.env}"
BIND_HOST="${MEDIAGEN_BIND_HOST:-172.20.0.1}"
BIND_PORT="${MEDIAGEN_BIND_PORT:-18080}"
BROWSER_PROXY="${MEDIAGEN_BROWSER_PROXY:-auto}"
CLASH_UPSTREAM="${MEDIAGEN_CLASH_UPSTREAM:-http://${BIND_HOST}:9090}"
CLASH_SECRET="${MEDIAGEN_CLASH_SECRET:-}"
NO_PROXY_VALUE="${MEDIAGEN_NO_PROXY:-127.0.0.1,localhost,::1,${BIND_HOST}}"
ADMIN_USERNAME="${MEDIAGEN_ADMIN_USERNAME:-admin}"
ADMIN_PASSWORD="${MEDIAGEN_ADMIN_PASSWORD:-}"
ADMIN_PASSWORD_HASH="${MEDIAGEN_ADMIN_PASSWORD_HASH:-}"
RUN_CHECKS="${MEDIAGEN_DEPLOY_RUN_CHECKS:-1}"
DEPLOY_NGINX="${MEDIAGEN_DEPLOY_NGINX:-1}"
NGINX_CONTAINER="${MEDIAGEN_NGINX_CONTAINER:-iflow-nginx-1}"
NGINX_CONF_HOST="${MEDIAGEN_NGINX_CONF_HOST:-/opt/iflow/nginx/conf.d/55-media-gen-realove-chat.conf}"
NGINX_CERT_HOST="${MEDIAGEN_NGINX_CERT_HOST:-/opt/iflow/nginx/certs}"
NGINX_CERT_CONTAINER="${MEDIAGEN_NGINX_CERT_CONTAINER:-/etc/nginx/certs}"
NGINX_ACME_SNIPPET="${MEDIAGEN_NGINX_ACME_SNIPPET:-/etc/nginx/snippets/acme-challenge.conf}"
ACME_HOME="${MEDIAGEN_ACME_HOME:-/root/.acme.sh}"
ACME_WEBROOT="${MEDIAGEN_ACME_WEBROOT:-/opt/iflow/acme/www}"
SSH_TARGET="${DEPLOY_USER}@${DEPLOY_HOST}"
SSH_OPTS=(-p "${DEPLOY_PORT}" -o BatchMode=yes -o StrictHostKeyChecking=accept-new)
GENERATED_PASSWORD="0"

log() {
  printf '\n\033[1;34m[deploy]\033[0m %s\n' "$*"
}

die() {
  printf '\n\033[1;31m[deploy][error]\033[0m %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "本机缺少命令：$1"
}

quote_remote() {
  printf '%q' "$1"
}

remote_env_prefix() {
  local output=""
  local item name value
  for item in "$@"; do
    name="${item%%=*}"
    value="${item#*=}"
    output+=" ${name}=$(quote_remote "${value}")"
  done
  printf '%s' "${output}"
}

remote() {
  ssh "${SSH_OPTS[@]}" "${SSH_TARGET}" "$@"
}

sha256_text() {
  local value="$1"
  VALUE_TO_HASH="${value}" node -e "const c=require('crypto');process.stdout.write(c.createHash('sha256').update(process.env.VALUE_TO_HASH || '').digest('hex'))"
}

generate_password() {
  node -e "process.stdout.write(require('crypto').randomBytes(30).toString('base64url'))"
}

prepare_admin_secret() {
  if [[ -z "${ADMIN_PASSWORD}" && -z "${ADMIN_PASSWORD_HASH}" ]]; then
    ADMIN_PASSWORD="$(generate_password)"
    GENERATED_PASSWORD="1"
  fi
  if [[ -z "${ADMIN_PASSWORD_HASH}" ]]; then
    ADMIN_PASSWORD_HASH="$(sha256_text "${ADMIN_PASSWORD}")"
  fi
}

print_config() {
  log "部署目标"
  cat <<INFO
服务器：${SSH_TARGET}
SSH 端口：${DEPLOY_PORT}
域名：https://${DEPLOY_DOMAIN}
远程源码：${REMOTE_SRC}
远程应用：${REMOTE_APP}
服务监听：${BIND_HOST}:${BIND_PORT}
默认浏览器代理：${BROWSER_PROXY}
管理员账号：${ADMIN_USERNAME}
INFO
}

run_local_checks() {
  if [[ "${RUN_CHECKS}" != "1" ]]; then
    log "已跳过本地检查：MEDIAGEN_DEPLOY_RUN_CHECKS=${RUN_CHECKS}"
    return
  fi

  log "运行本地快速检查"
  (cd "${ROOT_DIR}" && npm run type-check)
  (cd "${ROOT_DIR}" && cargo fmt --check --all)
  (cd "${ROOT_DIR}" && cargo test -p mediagen-browser-gateway)
}

sync_source() {
  log "同步源码到服务器"
  # 使用系统自带的 tar 通过 SSH 传输，避免新服务器必须预装 rsync。
  COPYFILE_DISABLE=1 tar -C "${ROOT_DIR}" -czf - \
    --exclude '.git/' \
    --exclude 'node_modules/' \
    --exclude 'target/' \
    --exclude 'dist-local/' \
    --exclude '.playwright-cli/' \
    --exclude 'coverage/' \
    --exclude '.env' \
    --exclude '.env.local' \
    --exclude '.env.*.local' \
    . | remote "rm -rf $(quote_remote "${REMOTE_SRC}") && mkdir -p $(quote_remote "${REMOTE_SRC}") && tar -xzf - -C $(quote_remote "${REMOTE_SRC}")"
}

remote_build_and_install() {
  log "服务器构建并安装 MediaGen"
  local env_prefix
  env_prefix="$(remote_env_prefix \
    "REMOTE_SRC=${REMOTE_SRC}" \
    "REMOTE_APP=${REMOTE_APP}" \
    "REMOTE_DATA=${REMOTE_DATA}" \
    "REMOTE_LOGS=${REMOTE_LOGS}" \
    "REMOTE_ENV_FILE=${REMOTE_ENV_FILE}" \
    "BIND_HOST=${BIND_HOST}" \
    "BIND_PORT=${BIND_PORT}" \
    "BROWSER_PROXY=${BROWSER_PROXY}" \
    "NO_PROXY_VALUE=${NO_PROXY_VALUE}" \
    "ADMIN_USERNAME=${ADMIN_USERNAME}" \
    "ADMIN_PASSWORD_HASH=${ADMIN_PASSWORD_HASH}")"

  remote "${env_prefix} bash -s" <<'REMOTE_SCRIPT'
set -euo pipefail

as_root() {
  if [[ "$(id -u)" == "0" ]]; then
    "$@"
  else
    sudo "$@"
  fi
}

install_packages() {
  apply_package_proxy
  if command -v dnf >/dev/null 2>&1; then
    install_rpm_packages dnf nodejs npm curl tar gzip unzip gcc gcc-c++ pkgconf-pkg-config openssl-devel ca-certificates \
      gtk3 nss alsa-lib libX11 libXcomposite libXdamage libXrandr mesa-libgbm atk at-spi2-atk cups-libs libdrm \
      google-noto-sans-cjk-ttc-fonts wqy-microhei-fonts
  elif command -v yum >/dev/null 2>&1; then
    install_rpm_packages yum nodejs npm curl tar gzip unzip gcc gcc-c++ pkgconfig openssl-devel ca-certificates \
      gtk3 nss alsa-lib libX11 libXcomposite libXdamage libXrandr mesa-libgbm atk at-spi2-atk cups-libs libdrm \
      google-noto-sans-cjk-ttc-fonts wqy-microhei-fonts
  elif command -v apt-get >/dev/null 2>&1; then
    as_root apt-get update
    as_root apt-get install -y nodejs npm curl tar gzip unzip build-essential pkg-config libssl-dev ca-certificates \
      libgtk-3-0 libnss3 libasound2 libx11-6 libxcomposite1 libxdamage1 libxrandr2 libgbm1 libatk-bridge2.0-0 libcups2 libdrm2
    as_root apt-get install -y fonts-noto-cjk fonts-wqy-microhei
  fi
  command -v fc-cache >/dev/null 2>&1 && as_root fc-cache -f || true
}

install_rpm_packages() {
  local package_manager="$1"
  shift
  local missing=() package
  for package in "$@"; do
    rpm -q "${package}" >/dev/null 2>&1 || missing+=("${package}")
  done
  if (( ${#missing[@]} > 0 )); then
    as_root "${package_manager}" install -y "${missing[@]}"
  fi
}

apply_package_proxy() {
  local proxy
  proxy="$(package_proxy_url || true)"
  if [[ -z "${proxy}" ]]; then
    return
  fi
  export http_proxy="${proxy}" https_proxy="${proxy}" HTTP_PROXY="${proxy}" HTTPS_PROXY="${proxy}"
}

package_proxy_url() {
  if [[ "${BROWSER_PROXY}" == "auto" ]] && bash -c '</dev/tcp/127.0.0.1/7890' >/dev/null 2>&1; then
    printf 'http://127.0.0.1:7890\n'
  elif [[ "${BROWSER_PROXY}" == http://* || "${BROWSER_PROXY}" == https://* ]]; then
    printf '%s\n' "${BROWSER_PROXY}"
  fi
}

cargo_is_modern() {
  command -v cargo >/dev/null 2>&1 || return 1
  local minor
  minor="$(cargo --version | awk '{split($2, v, "."); print v[2]}')"
  [[ "${minor:-0}" -ge 85 ]]
}

ensure_rust() {
  export PATH="${HOME}/.cargo/bin:${PATH}"
  if command -v rustup >/dev/null 2>&1; then
    rustup default stable
    return
  fi
  if cargo_is_modern; then
    return
  fi

  export RUSTUP_DIST_SERVER="${RUSTUP_DIST_SERVER:-https://mirrors.aliyun.com/rustup}"
  export RUSTUP_UPDATE_ROOT="${RUSTUP_UPDATE_ROOT:-https://mirrors.aliyun.com/rustup/rustup}"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  export PATH="${HOME}/.cargo/bin:${PATH}"
  rustup default stable
}

write_env_file() {
  local browser_proxy
  browser_proxy="$(resolve_browser_proxy)"
  as_root mkdir -p "$(dirname "${REMOTE_ENV_FILE}")"
  as_root tee "${REMOTE_ENV_FILE}" >/dev/null <<ENV_FILE
MEDIAGEN_HOST=${BIND_HOST}
MEDIAGEN_PORT=${BIND_PORT}
MEDIAGEN_CONSOLE_DIR=${REMOTE_APP}/console
MEDIAGEN_PLUGINS_DIR=${REMOTE_APP}/plugins
MEDIAGEN_DATA_DIR=${REMOTE_DATA}
MEDIAGEN_LOGS_DIR=${REMOTE_LOGS}
MEDIAGEN_PROFILES_DIR=${REMOTE_DATA}/profiles
CLOAK_BROWSER_BUNDLE_DIR=${REMOTE_APP}/browser-runtime
CLOAKBROWSER_CACHE_DIR=${REMOTE_DATA}/.cloakbrowser
MEDIAGEN_ADMIN_USERNAME=${ADMIN_USERNAME}
MEDIAGEN_ADMIN_PASSWORD_HASH=${ADMIN_PASSWORD_HASH}
ENV_FILE
  if [[ -n "${browser_proxy}" ]]; then
    write_proxy_env "${browser_proxy}"
  fi
  as_root chmod 600 "${REMOTE_ENV_FILE}"
}

write_proxy_env() {
  local proxy="$1"
  for key in MEDIAGEN_BROWSER_PROXY ALL_PROXY all_proxy HTTP_PROXY HTTPS_PROXY http_proxy https_proxy; do
    printf '%s=%s\n' "${key}" "${proxy}" | as_root tee -a "${REMOTE_ENV_FILE}" >/dev/null
  done
  printf 'NO_PROXY=%s\nno_proxy=%s\n' "${NO_PROXY_VALUE}" "${NO_PROXY_VALUE}" | as_root tee -a "${REMOTE_ENV_FILE}" >/dev/null
}

resolve_browser_proxy() {
  if [[ -z "${BROWSER_PROXY}" || "${BROWSER_PROXY}" == "none" ]]; then
    return
  fi
  if [[ "${BROWSER_PROXY}" != "auto" ]]; then
    printf '%s\n' "${BROWSER_PROXY}"
    return
  fi

  # mihomo/clash 常用 mixed-port。Chromium 用 socks5:// 更稳，DNS 也会交给代理处理。
  if bash -c '</dev/tcp/127.0.0.1/7890' >/dev/null 2>&1; then
    printf 'socks5://127.0.0.1:7890\n'
  fi
}

write_systemd_service() {
  as_root tee /etc/systemd/system/mediagen-server.service >/dev/null <<SERVICE
[Unit]
Description=MediaGen Server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=mediagen
Group=mediagen
WorkingDirectory=${REMOTE_APP}
EnvironmentFile=${REMOTE_ENV_FILE}
ExecStart=${REMOTE_APP}/bin/mediagen-server
Restart=always
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
SERVICE
}

install_packages
ensure_rust

if ! id mediagen >/dev/null 2>&1; then
  as_root useradd --system --home "${REMOTE_DATA}" --shell /sbin/nologin mediagen
fi

as_root mkdir -p "${REMOTE_APP}/bin" "${REMOTE_DATA}/profiles" "${REMOTE_LOGS}" /var/cache/mediagen
as_root chown -R mediagen:mediagen "${REMOTE_DATA}" "${REMOTE_LOGS}" /var/cache/mediagen

cd "${REMOTE_SRC}"
export CARGO_TARGET_DIR=/var/cache/mediagen/cargo-target
npm ci --ignore-scripts
npm run build:console
cargo build --release -p mediagen-server
if [[ ! -x "${REMOTE_APP}/browser-runtime/chrome" ]]; then
  node scripts/ensure-cloakbrowser-runtime.mjs \
    --output "${REMOTE_APP}/browser-runtime" \
    --cache /var/cache/mediagen/cloakbrowser
else
  echo "复用服务器已安装的 CloakBrowser 运行时"
fi

as_root install -m 0755 "${CARGO_TARGET_DIR}/release/mediagen-server" "${REMOTE_APP}/bin/mediagen-server"
as_root rm -rf "${REMOTE_APP}/console" "${REMOTE_APP}/plugins"
as_root cp -a apps/console/dist "${REMOTE_APP}/console"
as_root cp -a plugins "${REMOTE_APP}/plugins"
as_root chown -R mediagen:mediagen "${REMOTE_APP}" "${REMOTE_DATA}" "${REMOTE_LOGS}"

write_env_file
write_systemd_service
as_root systemctl daemon-reload
as_root systemctl enable mediagen-server
as_root systemctl restart mediagen-server
systemctl is-active mediagen-server
REMOTE_SCRIPT
}

configure_nginx() {
  if [[ "${DEPLOY_NGINX}" != "1" ]]; then
    log "已跳过 nginx 配置：MEDIAGEN_DEPLOY_NGINX=${DEPLOY_NGINX}"
    return
  fi

  log "配置 nginx、HTTP/2 和证书"
  local env_prefix
  env_prefix="$(remote_env_prefix \
    "DEPLOY_DOMAIN=${DEPLOY_DOMAIN}" \
    "BIND_HOST=${BIND_HOST}" \
    "BIND_PORT=${BIND_PORT}" \
    "CLASH_UPSTREAM=${CLASH_UPSTREAM}" \
    "CLASH_SECRET=${CLASH_SECRET}" \
    "NGINX_CONTAINER=${NGINX_CONTAINER}" \
    "NGINX_CONF_HOST=${NGINX_CONF_HOST}" \
    "NGINX_CERT_HOST=${NGINX_CERT_HOST}" \
    "NGINX_CERT_CONTAINER=${NGINX_CERT_CONTAINER}" \
    "NGINX_ACME_SNIPPET=${NGINX_ACME_SNIPPET}" \
    "ACME_HOME=${ACME_HOME}" \
    "ACME_WEBROOT=${ACME_WEBROOT}")"

  remote "${env_prefix} bash -s" <<'REMOTE_SCRIPT'
set -euo pipefail

fullchain_host="${NGINX_CERT_HOST}/${DEPLOY_DOMAIN}/fullchain.cer"
key_host="${NGINX_CERT_HOST}/${DEPLOY_DOMAIN}/${DEPLOY_DOMAIN}.key"
fullchain_container="${NGINX_CERT_CONTAINER}/${DEPLOY_DOMAIN}/fullchain.cer"
key_container="${NGINX_CERT_CONTAINER}/${DEPLOY_DOMAIN}/${DEPLOY_DOMAIN}.key"

docker exec "${NGINX_CONTAINER}" nginx -v >/dev/null
mkdir -p "$(dirname "${NGINX_CONF_HOST}")" "${NGINX_CERT_HOST}/${DEPLOY_DOMAIN}" "${ACME_WEBROOT}"

write_http_config() {
  cat >"${NGINX_CONF_HOST}" <<NGINX_HTTP
server {
  listen 80;
  server_name ${DEPLOY_DOMAIN};

  include ${NGINX_ACME_SNIPPET};

  location / {
    return 301 https://${DEPLOY_DOMAIN}\$request_uri;
  }
}
NGINX_HTTP
}

write_https_config() {
  cat >"${NGINX_CONF_HOST}" <<NGINX_HTTPS
server {
  listen 80;
  server_name ${DEPLOY_DOMAIN};

  include ${NGINX_ACME_SNIPPET};

  location / {
    return 301 https://${DEPLOY_DOMAIN}\$request_uri;
  }
}

server {
  listen 443 ssl;
  http2 on;
  server_name ${DEPLOY_DOMAIN};

  ssl_certificate ${fullchain_container};
  ssl_certificate_key ${key_container};
  include /etc/nginx/snippets/ssl-common.conf;
  client_max_body_size 100m;

  location = /_mediagen_clash_auth {
    internal;
    proxy_pass http://${BIND_HOST}:${BIND_PORT}/api/accounts;
    proxy_pass_request_body off;
    proxy_set_header Content-Length "";
    proxy_set_header Cookie \$http_cookie;
  }

  location = /clash {
    # 统一经过自动配置入口，确保面板始终携带 Clash 控制器地址和密钥。
    return 302 /clash-auto;
  }

  location = /clash-auto {
    auth_request /_mediagen_clash_auth;
    error_page 401 =302 /;
    error_page 418 = @mediagen_clash_auto;
    recursive_error_pages on;
    try_files /__mediagen_clash_auto__ =418;
  }

  location @mediagen_clash_auto {
    default_type text/html;
    add_header Cache-Control "no-store";
    return 200 '<!doctype html><meta charset="utf-8"><title>打开 Clash 面板</title><script>location.replace("/clash/#/setup?protocol=https&hostname=${DEPLOY_DOMAIN}&port=443&secondaryPath=%2Fclash-api&secret=${CLASH_SECRET}&type=clash&disableUpgradeCore=1")</script>';
  }

  location ^~ /clash/ {
    auth_request /_mediagen_clash_auth;
    error_page 401 =302 /;
    proxy_pass ${CLASH_UPSTREAM}/ui/;
    include /etc/nginx/snippets/proxy-common.conf;
  }

  location ^~ /clash-api/ {
    auth_request /_mediagen_clash_auth;
    proxy_pass ${CLASH_UPSTREAM}/;
    include /etc/nginx/snippets/proxy-common.conf;
  }

  location / {
    proxy_pass http://${BIND_HOST}:${BIND_PORT};
    # 公共代理片段已统一配置 WebSocket Upgrade 和超时。
    include /etc/nginx/snippets/proxy-common.conf;
  }
}
NGINX_HTTPS
}

reload_nginx() {
  docker exec "${NGINX_CONTAINER}" nginx -t
  docker exec "${NGINX_CONTAINER}" nginx -s reload
}

if [[ ! -s "${fullchain_host}" || ! -s "${key_host}" ]]; then
  write_http_config
  reload_nginx
  test -x "${ACME_HOME}/acme.sh"
  "${ACME_HOME}/acme.sh" --issue -d "${DEPLOY_DOMAIN}" -w "${ACME_WEBROOT}" --server letsencrypt
  "${ACME_HOME}/acme.sh" --install-cert -d "${DEPLOY_DOMAIN}" \
    --key-file "${key_host}" \
    --fullchain-file "${fullchain_host}" \
    --reloadcmd "docker exec ${NGINX_CONTAINER} nginx -s reload"
fi

write_https_config
reload_nginx
REMOTE_SCRIPT
}

verify_clash_anonymous() {
  [[ -n "${CLASH_SECRET}" ]] || return
  local base_url="$1" entry_url panel_status api_status
  entry_url="$(curl -sS --http2 -o /dev/null -w '%{redirect_url}' "${base_url}/clash")"
  panel_status="$(curl -sS --http2 -o /dev/null -w '%{http_code}' "${base_url}/clash/")"
  api_status="$(curl -sS --http2 -o /dev/null -w '%{http_code}' "${base_url}/clash-api/version")"
  [[ "${entry_url}" == "${base_url}/clash-auto" ]] || die "Clash 入口未指向自动配置页：${entry_url}"
  [[ "${panel_status}" == "302" ]] || die "Clash 面板未登录保护异常，返回 ${panel_status}"
  [[ "${api_status}" == "401" ]] || die "Clash API 未登录保护异常，返回 ${api_status}"
}

verify_clash_authenticated() {
  [[ -n "${CLASH_SECRET}" ]] || return
  local base_url="$1" cookie_file="$2"
  curl -fsS --http2 -b "${cookie_file}" "${base_url}/clash/" >/dev/null
  curl -fsS --http2 -b "${cookie_file}" -H "Authorization: Bearer ${CLASH_SECRET}" \
    "${base_url}/clash-api/version" >/dev/null
}

verify_deploy() {
  log "验证线上服务"
  local base_url="https://${DEPLOY_DOMAIN}"
  local cookie_file status_code login_body login_payload

  curl -fsS --http2 "${base_url}/health" >/dev/null
  status_code="$(curl -sS --http2 -o /tmp/mediagen-deploy-accounts.json -w '%{http_code}' "${base_url}/api/accounts")"
  [[ "${status_code}" == "401" ]] || die "未登录保护异常，/api/accounts 返回 ${status_code}"
  verify_clash_anonymous "${base_url}"

  if [[ -n "${ADMIN_PASSWORD}" ]]; then
    cookie_file="$(mktemp)"
    login_body="$(mktemp)"
    login_payload="$(
      LOGIN_USER="${ADMIN_USERNAME}" LOGIN_PASSWORD="${ADMIN_PASSWORD}" \
        node -e "console.log(JSON.stringify({username:process.env.LOGIN_USER,password:process.env.LOGIN_PASSWORD}))"
    )"
    curl -fsS --http2 -c "${cookie_file}" -b "${cookie_file}" \
      -H 'content-type: application/json' \
      -d "${login_payload}" \
      "${base_url}/api/auth/login" >"${login_body}"
    grep -q '"authenticated":true' "${login_body}" || die "管理员账号登录验证失败"
    curl -fsS --http2 -b "${cookie_file}" "${base_url}/api/plugins" >/dev/null
    verify_clash_authenticated "${base_url}" "${cookie_file}"
    rm -f "${cookie_file}" "${login_body}"
  else
    log "只提供了密码哈希，跳过登录验证"
  fi
}

print_success() {
  log "部署完成"
  cat <<SUCCESS
访问地址：https://${DEPLOY_DOMAIN}
管理员账号：${ADMIN_USERNAME}
SUCCESS
  if [[ -n "${ADMIN_PASSWORD}" ]]; then
    cat <<SUCCESS
管理员密码：${ADMIN_PASSWORD}
SUCCESS
  fi
  if [[ "${GENERATED_PASSWORD}" == "1" ]]; then
    cat <<'SUCCESS'

提示：本次密码是脚本自动生成的。
下次如果希望保持同一个密码，请把下面两项保存到 .env.deploy.local：
  MEDIAGEN_ADMIN_USERNAME=admin
  MEDIAGEN_ADMIN_PASSWORD=上面打印的密码
SUCCESS
  fi
}

main() {
  require_cmd ssh
  require_cmd tar
  require_cmd curl
  require_cmd node
  prepare_admin_secret
  print_config
  run_local_checks
  sync_source
  remote_build_and_install
  configure_nginx
  verify_deploy
  print_success
}

main "$@"
