#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

cd "$REPO_ROOT"

# Git hook 可能由 IDE/GUI 触发，PATH 会比普通终端少，这里补齐提交检查常用工具路径。
export PATH="$REPO_ROOT/node_modules/.bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if [[ -x "$REPO_ROOT/node_modules/.bin/lint-staged" ]]; then
  LINT_STAGED=("$REPO_ROOT/node_modules/.bin/lint-staged")
elif command -v lint-staged >/dev/null 2>&1; then
  LINT_STAGED=("lint-staged")
elif command -v npx >/dev/null 2>&1; then
  LINT_STAGED=("npx" "lint-staged")
else
  echo "❌ 找不到 lint-staged。请先在项目根目录执行 npm install，再重新提交。"
  exit 1
fi

# 统一承接 pre-commit 输出，避免用户只看到 lint-staged 失败却不知道该先看哪里。
if "${LINT_STAGED[@]}"; then
  exit 0
fi

echo
echo "❌ 提交前检查未通过，这次 commit 已被本地 pre-commit 拦下。"
echo "请优先看上面第一个带有 ✖ 或 error 的报错块，那一段就是直接失败原因。"
echo
echo "处理完成后，重新执行 git commit 即可。"

exit 1
