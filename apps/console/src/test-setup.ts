import '@testing-library/jest-dom/vitest'

function createMemoryStorage(): Storage {
  const store = new Map<string, string>()

  return {
    get length() {
      return store.size
    },
    clear() {
      store.clear()
    },
    getItem(key: string) {
      return store.get(key) ?? null
    },
    key(index: number) {
      return [...store.keys()][index] ?? null
    },
    removeItem(key: string) {
      store.delete(key)
    },
    setItem(key: string, value: string) {
      store.set(key, String(value))
    },
  }
}

function hasUsableStorage(storage: unknown): storage is Storage {
  return (
    typeof storage === 'object' &&
    storage !== null &&
    typeof (storage as Storage).clear === 'function' &&
    typeof (storage as Storage).getItem === 'function' &&
    typeof (storage as Storage).setItem === 'function'
  )
}

// Node 22 在某些启动参数下会暴露一个不完整的 localStorage。
// 测试里统一兜底成标准 Storage，避免业务测试被运行时差异干扰。
const testStorage = hasUsableStorage(globalThis.localStorage)
  ? globalThis.localStorage
  : createMemoryStorage()

Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: testStorage,
})

Object.defineProperty(window, 'localStorage', {
  configurable: true,
  value: testStorage,
})
