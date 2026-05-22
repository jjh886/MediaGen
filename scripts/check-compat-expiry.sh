#!/usr/bin/env bash

set -euo pipefail

# COMPAT 注释到期检查：过期即以 exit 1 阻断；CI 可用 --report-only 退回 warning。
# CLAUDE.md 要求兼容期到期必须删除，因此默认行为必须是 fail-fast。

REPORT_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --report-only)
      REPORT_ONLY=1
      shift
      ;;
    *)
      ;;
  esac
done

CURRENT_DATE="$(date +%Y-%m-%d)"
FOUND=0

while IFS= read -r line; do
  DATE="$(printf '%s' "$line" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' || true)"

  if [[ -n "$DATE" && "$DATE" < "$CURRENT_DATE" ]]; then
    echo "EXPIRED: $line" | tee /dev/stderr
    FOUND=1
  fi
done < <(
  rg -n '// COMPAT\(' \
    --glob '*.ts' \
    --glob '*.tsx' \
    --glob '*.go' \
    --glob '*.rs' \
    . 2>/dev/null || true
)

if [[ "$FOUND" -eq 1 ]]; then
  if [[ "$REPORT_ONLY" -eq 1 ]]; then
    echo "::warning::发现已过期的兼容代码，--report-only 模式下仅提示，未阻断。"
    exit 0
  fi
  echo "❌ 发现已过期的 COMPAT 注释，按约定必须在本轮迭代中清理" >&2
  exit 1
fi

echo "✅ 未发现过期的 COMPAT 注释"
