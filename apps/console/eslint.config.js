import js from '@eslint/js'
import typescriptPlugin from '@typescript-eslint/eslint-plugin'
import typescriptParser from '@typescript-eslint/parser'
import globals from 'globals'

const consoleSourceFiles = ['src/**/*.{ts,tsx}']

export default [
  {
    ignores: ['**/dist/**', '**/coverage/**', '**/src-tauri/**'],
  },
  {
    ...js.configs.recommended,
    files: consoleSourceFiles,
  },
  {
    files: consoleSourceFiles,
    languageOptions: {
      parser: typescriptParser,
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: globals.browser,
    },
    plugins: {
      '@typescript-eslint': typescriptPlugin,
    },
    rules: {
      ...typescriptPlugin.configs.recommended.rules,
      // TypeScript 编译器负责解析类型与声明，避免基础 JavaScript 规则误报 DOM/React 类型。
      'no-undef': 'off',
      // 保持原有门禁语义；变量有效性仍由 TypeScript 与 no-unused-vars 检查。
      'no-useless-assignment': 'off',
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      'no-console': ['warn', { allow: ['warn', 'error'] }],
    },
  },
]
