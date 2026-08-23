import * as jestDomMatchers from '@testing-library/jest-dom/matchers'
import { afterEach, beforeEach, expect, rs } from '@rstest/core'
import { cleanup } from '@testing-library/react'

expect.extend(jestDomMatchers)

// jsdom 无 matchMedia：全局 stub（默认深色，matches 恒 false），
// 测试浅色分支时自行 rs.stubGlobal 覆盖
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

// jsdom 的 WebSocket 是真实客户端：App 集成测试 mock 出 running: true 时，
// useClash → useWebSocket 会真的向 ws://localhost/api/clash/traffic 发起连接，
// 连接被拒后的退避重连（window.setTimeout + Math.random 抖动）与 fake timers 交错，
// 行为取决于运行环境。stub 成惰性实例（构造即 CLOSED，不联网、不触发任何事件），
// 需要测 WebSocket 行为时自行 rs.stubGlobal 覆盖。
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
// 放进 beforeEach：App.test 等文件的 afterEach 会 unstubAllGlobals，
// 模块顶层只 stub 一次会对后续用例失效
beforeEach(() => {
  rs.stubGlobal('WebSocket', WebSocketStub)
})

// Node ≥22 的内建 localStorage 在本环境返回 undefined（缺 --localstorage-file），
// 且遮蔽了 jsdom 的 window.localStorage —— 用内存 stub 顶替，每个用例前清空
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
