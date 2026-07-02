#!/bin/bash
# Codex Stop hook：质量门禁自动检查
#
# Codex 的 Stop hook 要求成功时 stdout 为空或输出 JSON；因此本脚本把门禁日志
# 暂存到临时文件，只在失败时写入关键摘要并用退出码 2 让 Codex 自动继续修复。
#
# 跳过方式：export SKIP_CODEX_QUALITY_GATE=1

set -uo pipefail

INPUT_FILE="$(mktemp -t mediagen-codex-quality-gate-input.XXXXXX)"
cleanup_input() {
  rm -f "$INPUT_FILE" 2>/dev/null || true
}
trap cleanup_input EXIT

# Codex 会通过 stdin 传入 hook JSON。保存下来用于多层 hook 入口去重。
cat >"$INPUT_FILE" || true

# 非交互 shell 下 PATH 不完整，补上 Homebrew 和 nvm
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"

export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then
  # shellcheck source=/dev/null
  . "$NVM_DIR/nvm.sh"
fi

if [ "${SKIP_CODEX_QUALITY_GATE:-}" = "1" ] || [ "${MEDIAGEN_SKIP_CODEX_QUALITY_GATE:-}" = "1" ]; then
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT" || exit 2

DEDUPE_KEY="$(
  node -e '
const fs = require("node:fs")
const crypto = require("node:crypto")
let input = {}
try {
  const raw = fs.readFileSync(process.argv[1], "utf8").trim()
  input = raw ? JSON.parse(raw) : {}
} catch {}
const sessionId = String(input.session_id || input.sessionId || "")
const turnId = String(input.turn_id || input.turnId || "")
if (sessionId && turnId) {
  process.stdout.write(crypto.createHash("sha1").update(`${sessionId}:${turnId}`).digest("hex"))
}
' "$INPUT_FILE" 2>/dev/null || true
)"

if [ -n "$DEDUPE_KEY" ]; then
  DEDUPE_DIR="$HOME/.codex/hook-state/mediagen-quality-gate/dedupe"
  mkdir -p "$DEDUPE_DIR" 2>/dev/null || true
  find "$DEDUPE_DIR" -type f -mtime +7 -delete 2>/dev/null || true
  DEDUPE_FILE="$DEDUPE_DIR/$DEDUPE_KEY"
  if ! (set -C; : >"$DEDUPE_FILE") 2>/dev/null; then
    exit 0
  fi
fi

collect_changed_files() {
  local tracked untracked
  tracked=$(git diff --name-only --diff-filter=ACMR HEAD -- 2>/dev/null || true)
  untracked=$(git ls-files --others --exclude-standard 2>/dev/null || true)
  printf '%s\n%s\n' "$tracked" "$untracked" | sed '/^[[:space:]]*$/d' | sort -u
}

CHANGED_FILES="$(collect_changed_files)"
CHANGED_FILES_JSON="$(printf '%s\n' "$CHANGED_FILES" | node -e 'const fs = require("node:fs"); const files = fs.readFileSync(0, "utf8").split(/\r?\n/).map((line) => line.trim()).filter(Boolean); process.stdout.write(JSON.stringify([...new Set(files)]));')"

has_changes() {
  local mod="$1"
  printf '%s\n' "$CHANGED_FILES" | grep -q "^${mod}/" 2>/dev/null
}

LOG_FILE="$(mktemp -t mediagen-codex-quality-gate.XXXXXX)"
FAILED_MODULES=""
CHECKED_MODULES=""

print_failure_summary() {
  MEDIAGEN_FAILED_MODULES="$FAILED_MODULES" node - "$LOG_FILE" <<'NODE'
const fs = require('node:fs')

const logPath = process.argv[2]
const failedModules = (process.env.MEDIAGEN_FAILED_MODULES || '').trim()
const text = fs.readFileSync(logPath, 'utf8')
const lines = text.replace(/\u001B\[[0-9;]*m/g, '').split(/\r?\n/)
const selected = []
const seen = new Set()

function keyFor(line) {
  return line.replace(/^\s*-\s+/, '').trim()
}

function add(line = '') {
  const value = line.trimEnd()
  if (!value) {
    if (selected[selected.length - 1] !== '') selected.push('')
    return
  }
  const key = keyFor(value)
  if (seen.has(key)) return
  seen.add(key)
  selected.push(value)
}

for (let index = 0; index < lines.length; index += 1) {
  const line = lines[index]
  const trimmed = line.trim()

  if (/^\[start-quality\]\[error\]/.test(trimmed)) {
    add(trimmed)
    continue
  }

  if (/^\[start-quality\].*失败摘录/.test(trimmed)) {
    const excerptLines = []
    for (let next = index + 1; next < lines.length; next += 1) {
      const nextLine = lines[next].trimEnd()
      if (!nextLine.trim() || /^\[start-quality\]/.test(nextLine.trim())) break
      if (/^\s*- /.test(nextLine) && !seen.has(keyFor(nextLine))) {
        excerptLines.push(nextLine)
      }
    }
    if (excerptLines.length > 0) {
      add()
      add(trimmed)
      for (const excerptLine of excerptLines) add(excerptLine)
    }
    continue
  }

  if (
    /发现.*(偏高|失败|重复|未覆盖)/.test(trimmed) ||
    /已阻断启动/.test(trimmed) ||
    /门禁未通过/.test(trimmed) ||
    /\b(error|failed|panic)\b/i.test(trimmed)
  ) {
    add(trimmed)
    continue
  }

  if (/^\s+\/.*:\d+/.test(line)) {
    add(line)
  }
}

const summary = selected.filter((line, index, list) => {
  if (line !== '') return true
  return index > 0 && index < list.length - 1
})

console.error('')
console.error('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
console.error(`[codex-hook] 门禁未通过的模块: ${failedModules}`)
console.error('[codex-hook] 关键错误摘录:')
console.error('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')

if (summary.length > 0) {
  for (const line of summary.slice(0, 50)) {
    console.error(line)
  }
} else {
  for (const line of lines.filter(Boolean).slice(-30)) {
    console.error(line)
  }
}

console.error('')
console.error(`[codex-hook] 完整日志已保留: ${logPath}`)
console.error('[codex-hook] 请根据上方关键错误自动修复后重试。')
NODE
}

run_module_gate() {
  local mod="$1"
  CHECKED_MODULES="$CHECKED_MODULES $mod"

  {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "[codex-hook] $mod 模块有改动，执行门禁检查..."
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  } >>"$LOG_FILE"

  if ! MEDIAGEN_CHANGED_FILES_JSON="$CHANGED_FILES_JSON" node scripts/gates/run-start-quality-gate.mjs --modules "$mod" --include-coverage >>"$LOG_FILE" 2>&1; then
    FAILED_MODULES="$FAILED_MODULES $mod"
  fi
}

if printf '%s\n' "$CHANGED_FILES" | grep -Eq '^(apps|crates|plugins|packages|scripts)/' 2>/dev/null; then
  run_module_gate "app"
fi

if [ -z "$CHECKED_MODULES" ]; then
  rm -f "$LOG_FILE" 2>/dev/null
  exit 0
fi

if [ -n "$FAILED_MODULES" ]; then
  print_failure_summary
  exit 2
fi

rm -f "$LOG_FILE" 2>/dev/null
exit 0
