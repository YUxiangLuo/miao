import { render } from '@testing-library/react'
import { beforeEach, describe, expect, it } from '@rstest/core'
import { useFlipContainer } from './useFlip'
import { FLIP_MS } from '../tokens'

// jsdom 的 offsetTop 恒 0：用 defineProperty 在元素实例上模拟布局。
// 注意时序——hook 在 render/rerender 的 layout effect 里测量，
// 所以必须先 setOffset 再 rerender，effect 才能量到「新位置」。
function setOffset(el: HTMLElement, top: number, left = 0) {
  Object.defineProperty(el, 'offsetTop', { value: top, configurable: true })
  Object.defineProperty(el, 'offsetLeft', { value: left, configurable: true })
}

interface AnimateCall {
  el: HTMLElement
  keyframes: Keyframe[]
  options: KeyframeAnimationOptions
}

let animateCalls: AnimateCall[] = []

function row(container: HTMLElement, key: string): HTMLElement {
  return container.querySelector(`[data-flip-key="${key}"]`) as HTMLElement
}

function layoutAs(container: HTMLElement, tops: Record<string, number>) {
  for (const [key, top] of Object.entries(tops)) setOffset(row(container, key), top)
}

function FlipList({ keys, active = true, show = true }: { keys: string[]; active?: boolean; show?: boolean }) {
  const ref = useFlipContainer<HTMLDivElement>(active)
  return (
    <div>
      {show ? (
        <div ref={ref} data-testid="container">
          {keys.map((key) => (
            <div key={key} data-flip-key={key}>{key}</div>
          ))}
        </div>
      ) : null}
    </div>
  )
}

describe('useFlipContainer', () => {
  beforeEach(() => {
    animateCalls = []
    Element.prototype.animate = function (this: HTMLElement, keyframes: Keyframe[], options: KeyframeAnimationOptions) {
      animateCalls.push({ el: this, keyframes, options })
      return {} as Animation
    } as typeof Element.prototype.animate
  })

  it('records positions on first render without animating', () => {
    render(<FlipList keys={['a', 'b', 'c']} />)
    expect(animateCalls).toHaveLength(0)
  })

  it('slides moved rows from their previous position (Invert + Play)', () => {
    const { getByTestId, rerender } = render(<FlipList keys={['a', 'b', 'c']} />)
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50, c: 100 })
    rerender(<FlipList keys={['a', 'b', 'c']} />) // hook 记录当前布局 a=0 b=50 c=100
    animateCalls = []

    // c 反超到顶：先让新布局生效（元素实例按 key 复用），再触发渲染
    layoutAs(container, { c: 0, b: 50, a: 100 })
    rerender(<FlipList keys={['c', 'b', 'a']} />)

    expect(animateCalls).toHaveLength(2)
    const byKey = new Map(animateCalls.map((call) => [call.el.dataset.flipKey as string, call]))
    // a 从 0 挪到 100：先视觉放回旧位置（-100）再滑到 0
    expect(byKey.get('a')?.keyframes).toEqual([
      { transform: 'translate(0px, -100px)' },
      { transform: 'translate(0, 0)' },
    ])
    expect(byKey.get('a')?.options).toEqual({ duration: FLIP_MS, easing: 'ease' })
    // c 从 100 挪到 0
    expect(byKey.get('c')?.keyframes).toEqual([
      { transform: 'translate(0px, 100px)' },
      { transform: 'translate(0, 0)' },
    ])
    // b 未动，不播
    expect(byKey.has('b')).toBe(false)
  })

  it('does not animate when positions are unchanged', () => {
    const { getByTestId, rerender } = render(<FlipList keys={['a', 'b']} />)
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50 })
    rerender(<FlipList keys={['a', 'b']} />)
    animateCalls = []
    rerender(<FlipList keys={['a', 'b']} />)
    expect(animateCalls).toHaveLength(0)
  })

  it('skips animation when prefers-reduced-motion is set', () => {
    // setupTests 只在 matchMedia 缺失时补 stub，这里手动替换后必须恢复，避免泄漏给后续用例
    const original = window.matchMedia
    window.matchMedia = (query: string) => ({
      matches: query === '(prefers-reduced-motion: reduce)',
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      onchange: null,
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList

    try {
      const { getByTestId, rerender } = render(<FlipList keys={['a', 'b']} />)
      const container = getByTestId('container')
      layoutAs(container, { a: 0, b: 50 })
      rerender(<FlipList keys={['a', 'b']} />)
      animateCalls = []
      layoutAs(container, { b: 0, a: 50 })
      rerender(<FlipList keys={['b', 'a']} />)
      expect(animateCalls).toHaveLength(0)
    } finally {
      window.matchMedia = original
    }
  })

  it('clears recorded positions while inactive so reopening starts fresh', () => {
    const { getByTestId, rerender } = render(<FlipList keys={['a', 'b']} />)
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50 })
    rerender(<FlipList keys={['a', 'b']} />)
    rerender(<FlipList keys={['a', 'b']} active={false} />)
    animateCalls = []
    layoutAs(container, { b: 0, a: 50 })
    rerender(<FlipList keys={['b', 'a']} />)
    expect(animateCalls).toHaveLength(0)
  })

  it('clears recorded positions when the rows container unmounts (empty state)', () => {
    const { getByTestId, rerender } = render(<FlipList keys={['a', 'b']} />)
    layoutAs(getByTestId('container'), { a: 0, b: 50 })
    rerender(<FlipList keys={['a', 'b']} />)
    rerender(<FlipList keys={['a', 'b']} show={false} />) // 空态：行容器卸载
    animateCalls = []
    rerender(<FlipList keys={['b', 'a']} />) // 容器重挂，从空白开始
    expect(animateCalls).toHaveLength(0)
  })

  it('stays silent when WAAPI is unavailable', () => {
    // @ts-expect-error 模拟无 WAAPI 的环境（如 jsdom）
    delete Element.prototype.animate
    const { getByTestId, rerender } = render(<FlipList keys={['a', 'b']} />)
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50 })
    rerender(<FlipList keys={['a', 'b']} />)
    expect(() => {
      layoutAs(container, { b: 0, a: 50 })
      rerender(<FlipList keys={['b', 'a']} />)
    }).not.toThrow()
  })
})
