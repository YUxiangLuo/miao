import { describe, expect, it } from '@rstest/core'
import { foldIcons } from './iconFold'

describe('foldIcons', () => {
  it('shows everything when all icons fit in one row', () => {
    expect(foldIcons(3, 10)).toEqual({ shown: 3, more: 0 })
    expect(foldIcons(4, 4)).toEqual({ shown: 4, more: 0 })
  })

  it('reserves the last cell for +N when icons overflow the row', () => {
    expect(foldIcons(5, 4)).toEqual({ shown: 3, more: 2 })
    expect(foldIcons(6, 2)).toEqual({ shown: 1, more: 5 })
  })

  it('caps the row at MAX_ICONS and folds the rest into +N', () => {
    // 区域够宽但超上限：显示 18 个，剩余进 +N
    expect(foldIcons(25, 20)).toEqual({ shown: 18, more: 7 })
    // 区域窄且超上限：末格让给 +N，more 含超上限部分
    expect(foldIcons(25, 5)).toEqual({ shown: 4, more: 21 })
  })

  it('always keeps at least one icon even when a cell barely fits', () => {
    expect(foldIcons(2, 1)).toEqual({ shown: 1, more: 1 })
  })
})
