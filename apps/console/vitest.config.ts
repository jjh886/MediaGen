import path from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    include: ['src/**/*.{test,spec}.{ts,tsx}', 'scripts/**/*.{test,spec}.mjs'],
    environment: 'jsdom',
    environmentOptions: {
      // 给 jsdom 固定来源，避免 localStorage 在 opaque origin 下不可用。
      jsdom: { url: 'http://localhost/' },
    },
    setupFiles: ['./src/test-setup.ts'],
    globals: true,
  },
})
