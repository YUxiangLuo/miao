import { describe, expect, it } from 'vitest'
import { isClashProxyGroup } from './useClash.js'

describe('isClashProxyGroup', () => {
  it('accepts selector and urltest groups from clash api', () => {
    expect(isClashProxyGroup('Selector')).toBe(true)
    expect(isClashProxyGroup('URLTest')).toBe(true)
    expect(isClashProxyGroup('Direct')).toBe(false)
    expect(isClashProxyGroup(undefined)).toBe(false)
  })
})
