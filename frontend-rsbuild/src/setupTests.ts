import * as jestDomMatchers from '@testing-library/jest-dom/matchers'
import { afterEach, beforeEach, expect, rs } from '@rstest/core'
import { cleanup } from '@solidjs/testing-library'

expect.extend(jestDomMatchers)

// jsdom 无 matchMedia：全局 stub（matches 恒 false），
// 测试特定媒体查询分支时自行 rs.stubGlobal 覆盖
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

// jsdom 的 WebSocket 是真实客户端：stub 成惰性实例避免测试联网与重连干扰
class WebSocketStub {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3
  readonly readyState = WebSocketStub.CLOSED
  onopen: (() => void) | null = null
  onclose: (() => void) | null = null
  onerror: (() => void) | null = null
  onmessage: (() => void) | null = null
  close() {}
}

beforeEach(() => {
  rs.stubGlobal('WebSocket', WebSocketStub)
})

// Node ≥22 的内建 localStorage 在本环境可能遮蔽 jsdom 的 window.localStorage —— 用内存 stub 顶替
const store = new Map<string, string>()
rs.stubGlobal('localStorage', {
  getItem: (k: string) => (store.has(k) ? store.get(k)! : null),
  setItem: (k: string, v: string) => store.set(k, String(v)),
  removeItem: (k: string) => store.delete(k),
  clear: () => store.clear(),
})
beforeEach(() => store.clear())

afterEach(() => {
  cleanup()
})
