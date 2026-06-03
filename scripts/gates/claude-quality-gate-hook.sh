#!/bin/bash
# Claude Code Stop hook：质量门禁自动检查
#
# 在 Claude 每次完成回复后，自动检测哪些模块有改动，并执行该模块对应的门禁检查。
# MediaGen 是独立桌面应用，检查范围集中在 src、src-tauri、plugins 和 scripts。
#
# 如果门禁失败（非零退出），Claude 会自动看到错误输出并尝试修复。
# 跳过方式：export SKIP_CLAUDE_QUALITY_GATE=1

set -uo pipefail

# 非交互 shell 下 PATH 不完整，补上 Homebrew 和 nvm
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"

export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
if [ -s "$NVM_DIR/nvm.sh" ]; then
  # shellcheck source=/dev/null
  . "$NVM_DIR/nvm.sh"
fi

# 允许通过环境变量跳过
if [ "${SKIP_CLAUDE_QUALITY_GATE:-}" = "1" ] || [ "${MEDIAGEN_SKIP_CLAUDE_QUALITY_GATE:-}" = "1" ]; then
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# ─── 会话级文件追踪 ───
# 优先使用 PostToolUse hook 记录的本次会话修改文件，避免多会话并行时互相干扰。
# 如果追踪文件不存在（如只用了 Bash 修改文件），则回退到 git diff 全量检测。
SESSION_KEY="${CLAUDE_SESSION_ID:-ppid_$PPID}"
SESSION_TRACK_FILE="/tmp/claude-session-${SESSION_KEY}-files.txt"

if [ -f "$SESSION_TRACK_FILE" ]; then
  # 将追踪到的绝对路径转为相对路径，去重
  SESSION_FILES=$(sort -u "$SESSION_TRACK_FILE" | while IFS= read -r f; do
    # 转为相对于仓库根目录的路径
    echo "${f#$REPO_ROOT/}"
  done)
  USE_SESSION_TRACKING=1
else
  USE_SESSION_TRACKING=0
fi

SESSION_FILES_JSON=""
if [ "$USE_SESSION_TRACKING" = "1" ]; then
  SESSION_FILES_JSON=$(printf '%s\n' "$SESSION_FILES" | node -e 'const fs = require("node:fs"); const files = fs.readFileSync(0, "utf8").split(/\r?\n/).map((line) => line.trim()).filter(Boolean); process.stdout.write(JSON.stringify([...new Set(files)]));')
fi

has_app_changes() {
  local pattern='^(src|src-tauri|plugins|scripts)/'
  if [ "$USE_SESSION_TRACKING" = "1" ]; then
    echo "$SESSION_FILES" | grep -Eq "$pattern" 2>/dev/null
  else
    local tracked untracked
    tracked=$(git diff --name-only HEAD -- src src-tauri plugins scripts 2>/dev/null || true)
    untracked=$(git ls-files --others --exclude-standard -- src src-tauri plugins scripts 2>/dev/null || true)
    [ -n "$tracked" ] || [ -n "$untracked" ]
  fi
}

FAILED_MODULES=""

# ─── App 门禁 ───
if has_app_changes; then
  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "[claude-hook] MediaGen App 有改动，执行门禁检查..."
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  if ! MEDIAGEN_CHANGED_FILES_JSON="$SESSION_FILES_JSON" node scripts/gates/run-start-quality-gate.mjs --modules app; then
    FAILED_MODULES="$FAILED_MODULES app"
  fi
fi

# ─── 汇总结果 ───
if [ -n "$FAILED_MODULES" ]; then
  echo ""
  echo "=============================================="
  echo "[claude-hook] 门禁未通过的模块:$FAILED_MODULES"
  echo "[claude-hook] 请根据上方错误输出自动修复后重试。"
  echo "=============================================="
  # 退出码 2 = 阻塞式失败，Claude 会中断停止并自动尝试修复
  rm -f "$SESSION_TRACK_FILE" 2>/dev/null
  exit 2
fi

# 清理会话追踪文件
rm -f "$SESSION_TRACK_FILE" 2>/dev/null
