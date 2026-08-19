import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, vi } from 'vitest'
import { cleanup } from '@testing-library/react'

// jsdom 无 matchMedia：全局 stub（默认深色，matches 恒 false），
// 测试浅色分支时自行 vi.stubGlobal 覆盖
beforeEach(() => {
  if (typeof window.matchMedia !== 'function') {
    window.matchMedia = (query) => ({
      matches: false,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      onchange: null,
      dispatchEvent: () => false,
    })
  }
})

// Node ≥22 的内建 localStorage 在本环境返回 undefined（缺 --localstorage-file），
// 且遮蔽了 jsdom 的 window.localStorage —— 用内存 stub 顶替，每个用例前清空
const store = new Map<string, string>()
vi.stubGlobal('localStorage', {
  getItem: (k: string) => (store.has(k) ? store.get(k)! : null),
  setItem: (k: string, v: string) => store.set(k, String(v)),
  removeItem: (k: string) => store.delete(k),
  clear: () => store.clear(),
})
beforeEach(() => store.clear())

afterEach(() => {
  cleanup()
})
