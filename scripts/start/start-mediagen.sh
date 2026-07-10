#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$SCRIPT_DIR"
if [[ ! -x "$ROOT_DIR/bin/mediagen-server" && -x "$SCRIPT_DIR/../../bin/mediagen-server" ]]; then
  ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

SERVER="$ROOT_DIR/bin/mediagen-server"
if [[ ! -x "$SERVER" ]]; then
  echo "没有找到 mediagen-server：$SERVER"
  exit 1
fi

if [[ -z "${MEDIAGEN_DATA_DIR:-}" ]]; then
  DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
  export MEDIAGEN_DATA_DIR="$DATA_HOME/mediagen"
fi
export MEDIAGEN_LOGS_DIR="${MEDIAGEN_LOGS_DIR:-$MEDIAGEN_DATA_DIR/logs}"
export MEDIAGEN_PROFILES_DIR="${MEDIAGEN_PROFILES_DIR:-$MEDIAGEN_DATA_DIR/profiles}"
mkdir -p "$MEDIAGEN_DATA_DIR" "$MEDIAGEN_LOGS_DIR" "$MEDIAGEN_PROFILES_DIR"

is_healthy() {
  curl -fsS "http://127.0.0.1:$1/health" >/dev/null 2>&1
}

is_port_open() {
  (echo >/dev/tcp/127.0.0.1/"$1") >/dev/null 2>&1
}

PORT="${MEDIAGEN_PORT:-}"
if [[ -z "$PORT" ]]; then
  for candidate in {18080..18089}; do
    if is_healthy "$candidate" || ! is_port_open "$candidate"; then
      PORT="$candidate"
      break
    fi
  done
fi

if [[ -z "$PORT" ]]; then
  echo "端口 18080-18089 都被占用，请先关闭旧的 MediaGen 或其他占用端口的程序"
  exit 1
fi

if [[ -z "${CLOAK_BROWSER_PATH:-}" ]]; then
  for browser in \
    "$ROOT_DIR/browser-runtime/Chromium.app/Contents/MacOS/Chromium" \
    "$ROOT_DIR/browser-runtime/chrome" \
    "$ROOT_DIR/browser-runtime/cloakbrowser/chrome" \
    "$ROOT_DIR/browser-runtime/cloakbrowser"; do
    if [[ -x "$browser" ]]; then
      export CLOAK_BROWSER_PATH="$browser"
      break
    fi
  done
fi

if ! is_healthy "$PORT"; then
  MEDIAGEN_PORT="$PORT" \
  MEDIAGEN_DATA_DIR="$MEDIAGEN_DATA_DIR" \
  MEDIAGEN_LOGS_DIR="$MEDIAGEN_LOGS_DIR" \
  MEDIAGEN_PROFILES_DIR="$MEDIAGEN_PROFILES_DIR" \
  MEDIAGEN_PLUGINS_DIR="$ROOT_DIR/plugins" \
  MEDIAGEN_CONSOLE_DIR="$ROOT_DIR/console" \
  "$SERVER" >>"$MEDIAGEN_LOGS_DIR/startup.log" 2>&1 &
fi

for _ in {1..150}; do
  if is_healthy "$PORT"; then
    if command -v xdg-open >/dev/null 2>&1; then
      xdg-open "http://127.0.0.1:$PORT" >/dev/null 2>&1 || true
    elif command -v open >/dev/null 2>&1; then
      open "http://127.0.0.1:$PORT" >/dev/null 2>&1 || true
    fi
    echo "MediaGen 已启动：http://127.0.0.1:$PORT"
    exit 0
  fi
  sleep 0.3
done

echo "MediaGen 启动超时，请查看 $MEDIAGEN_LOGS_DIR/startup.log"
exit 1
