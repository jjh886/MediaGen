// lint-staged 会把命中的文件路径追加到普通命令后面。
// cargo fmt --all 本身会格式化整个 Rust workspace，不能再接单个文件路径，所以这里用函数阻止追加参数。
const formatRustWorkspace = () => 'cargo fmt --all'

export default {
  'apps/console/src/**/*.{ts,tsx}': [
    'prettier --write',
    'bash -lc \'node scripts/check-code-line-limit.mjs --scope app --max 500 && node scripts/check-import-boundaries.mjs && npm --workspace @mediagen/console exec eslint -- --fix --max-warnings 0 "$@"\' --',
  ],
  '{apps/{server,local-launcher}/src,crates}/**/*.rs': formatRustWorkspace,
  'plugins/**/*.{js,json}': ['prettier --write'],
  '**/*.{json,md,yml,yaml,css}': ['prettier --write'],
}
