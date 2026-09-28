#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

cd "$REPO_ROOT"

# 保留当前 Node 的优先级，Homebrew 等路径仅用于补齐 IDE/GUI 缺失的工具。
export PATH="$REPO_ROOT/node_modules/.bin:$PATH:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin"

# IDE/GUI 未加载可用 Node 时，尝试使用本机 nvm 的默认版本。
if ! (node --version && :) >/dev/null 2>&1; then
  export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
  if [[ -s "$NVM_DIR/nvm.sh" ]]; then
    # nvm 初始化不兼容 nounset，完成后恢复严格检查。
    set +u
    # shellcheck source=/dev/null
    { . "$NVM_DIR/nvm.sh" --no-use && nvm use --silent default >/dev/null 2>&1; } || true
    set -u
  fi
fi

if ! (node --version && :) >/dev/null 2>&1; then
  echo "❌ Node.js 无法运行。请检查 node --version 的报错，修复 Node.js 安装后重新提交。"
  exit 1
fi

if [[ -x "$REPO_ROOT/node_modules/.bin/lint-staged" ]]; then
  LINT_STAGED=("$REPO_ROOT/node_modules/.bin/lint-staged")
elif command -v lint-staged >/dev/null 2>&1; then
  LINT_STAGED=("lint-staged")
elif command -v npx >/dev/null 2>&1; then
  LINT_STAGED=("npx" "lint-staged")
else
  echo "❌ 找不到 lint-staged。请先在项目根目录执行 npm ci，再重新提交。"
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
