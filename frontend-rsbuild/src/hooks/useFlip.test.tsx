import { render } from '@solidjs/testing-library'
import { For, Show, createSignal, flush } from 'solid-js'
import { beforeEach, describe, expect, it } from '@rstest/core'
import { useFlipContainer } from './useFlip'
import { FLIP_EASING, FLIP_MS } from '../tokens'

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

function createHarness(initialKeys: string[]) {
  const [keys, setKeys] = createSignal(initialKeys)
  const [active, setActive] = createSignal(true)
  const [show, setShow] = createSignal(true)
  const [tick, setTick] = createSignal(0)

  const FlipList = () => {
    const ref = useFlipContainer<HTMLDivElement>(active, () => `${tick()}:${keys().join(',')}`)
    return (
      <div>
        <Show when={show()}>
          <div ref={ref} data-testid="container">
            <For each={keys()}>
              {(key) => <div data-flip-key={key}>{key}</div>}
            </For>
          </div>
        </Show>
      </div>
    )
  }

  const view = render(() => <FlipList />)
  flush()

  return {
    ...view,
    update(next: { keys?: string[]; active?: boolean; show?: boolean }) {
      if (next.keys !== undefined) setKeys(next.keys)
      if (next.active !== undefined) setActive(next.active)
      if (next.show !== undefined) setShow(next.show)
      setTick((t) => t + 1)
      flush()
    },
  }
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
    createHarness(['a', 'b', 'c'])
    expect(animateCalls).toHaveLength(0)
  })

  it('slides moved rows from their previous position (Invert + Play)', () => {
    const { getByTestId, update } = createHarness(['a', 'b', 'c'])
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50, c: 100 })
    update({ keys: ['a', 'b', 'c'] })
    animateCalls = []

    layoutAs(container, { c: 0, b: 50, a: 100 })
    update({ keys: ['c', 'b', 'a'] })

    expect(animateCalls).toHaveLength(2)
    const byKey = new Map(animateCalls.map((call) => [call.el.dataset.flipKey as string, call]))
    expect(byKey.get('a')?.keyframes).toEqual([
      { transform: 'translate(0px, -100px)' },
      { transform: 'translate(0, 0)' },
    ])
    expect(byKey.get('a')?.options).toEqual({ duration: FLIP_MS, easing: FLIP_EASING })
    expect(byKey.get('c')?.keyframes).toEqual([
      { transform: 'translate(0px, 100px)' },
      { transform: 'translate(0, 0)' },
    ])
    expect(byKey.has('b')).toBe(false)
  })

  it('does not animate when positions are unchanged', () => {
    const { getByTestId, update } = createHarness(['a', 'b'])
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50 })
    update({ keys: ['a', 'b'] })
    animateCalls = []
    update({ keys: ['a', 'b'] })
    expect(animateCalls).toHaveLength(0)
  })

  it('skips animation when prefers-reduced-motion is set', () => {
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
      const { getByTestId, update } = createHarness(['a', 'b'])
      const container = getByTestId('container')
      layoutAs(container, { a: 0, b: 50 })
      update({ keys: ['a', 'b'] })
      animateCalls = []
      layoutAs(container, { b: 0, a: 50 })
      update({ keys: ['b', 'a'] })
      expect(animateCalls).toHaveLength(0)
    } finally {
      window.matchMedia = original
    }
  })

  it('clears recorded positions while inactive so reopening starts fresh', () => {
    const { getByTestId, update } = createHarness(['a', 'b'])
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50 })
    update({ keys: ['a', 'b'] })
    update({ active: false })
    animateCalls = []
    layoutAs(container, { b: 0, a: 50 })
    update({ active: true, keys: ['b', 'a'] })
    expect(animateCalls).toHaveLength(0)
  })

  it('clears recorded positions when the rows container unmounts (empty state)', () => {
    const { getByTestId, update } = createHarness(['a', 'b'])
    layoutAs(getByTestId('container'), { a: 0, b: 50 })
    update({ keys: ['a', 'b'] })
    update({ show: false })
    animateCalls = []
    update({ show: true, keys: ['b', 'a'] })
    expect(animateCalls).toHaveLength(0)
  })

  it('stays silent when WAAPI is unavailable', () => {
    // @ts-expect-error 模拟无 WAAPI 的环境
    delete Element.prototype.animate
    const { getByTestId, update } = createHarness(['a', 'b'])
    const container = getByTestId('container')
    layoutAs(container, { a: 0, b: 50 })
    update({ keys: ['a', 'b'] })
    expect(() => {
      layoutAs(container, { b: 0, a: 50 })
      update({ keys: ['b', 'a'] })
    }).not.toThrow()
  })
})
