#!/usr/bin/env bash

set -euo pipefail

# 统一承接 pre-commit 输出，避免用户只看到 lint-staged 失败却不知道该先看哪里。
if npx lint-staged; then
  exit 0
fi

echo
echo "❌ 提交前检查未通过，这次 commit 已被本地 pre-commit 拦下。"
echo "请优先看上面第一个带有 ✖ 或 error 的报错块，那一段就是直接失败原因。"
echo
echo "处理完成后，重新执行 git commit 即可。"

exit 1
