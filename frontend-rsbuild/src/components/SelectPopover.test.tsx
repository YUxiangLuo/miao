import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from './SelectPopover'

const FLAG_US = '\u{1F1FA}\u{1F1F8}'
const FLAG_JP = '\u{1F1EF}\u{1F1F5}'
const FLAG_HK = '\u{1F1ED}\u{1F1F0}'
const FLAG_SG = '\u{1F1F8}\u{1F1EC}'
const FLAG_TW = '\u{1F1F9}\u{1F1FC}'
const FLAG_GB = '\u{1F1EC}\u{1F1E7}'
const FLAG_DE = '\u{1F1E9}\u{1F1EA}'
const FLAG_FR = '\u{1F1EB}\u{1F1F7}'
const FLAG_KR = '\u{1F1F0}\u{1F1F7}'
const FLAG_AE = '\u{1F1E6}\u{1F1EA}'
const FLAG_CA = '\u{1F1E8}\u{1F1E6}'
const FLAG_AU = '\u{1F1E6}\u{1F1FA}'
const FLAG_NL = '\u{1F1F3}\u{1F1F1}'
const PICTO_GLOBE = '\u{1F310}'
const PICTO_LOCK = '\u{1F512}'
const PICTO_BOLT = '\u26A1'
const PICTO_STOP = '\u26D4'

describe('cleanNodeDisplay', () => {
  it('extracts ISO country codes from regional flag pairs and strips flags from name', () => {
    const cases: Array<[string, string, string]> = [
      [`${FLAG_US} IPDeep 洛杉矶静态住宅`, 'US', 'IPDeep 洛杉矶静态住宅'],
      [`${FLAG_JP} 日本高速专线 01`, 'JP', '日本高速专线 01'],
      [`${FLAG_HK} 香港 IEPL 02`, 'HK', '香港 IEPL 02'],
      [`${FLAG_SG} 新加坡 BGP`, 'SG', '新加坡 BGP'],
      [`${FLAG_TW} 台北原生`, 'TW', '台北原生'],
      [`${FLAG_GB} 伦敦商宽`, 'GB', '伦敦商宽'],
      [`${FLAG_DE} 法兰克福`, 'DE', '法兰克福'],
      [`${FLAG_FR} 巴黎节点`, 'FR', '巴黎节点'],
      [`${FLAG_KR} 首尔专线`, 'KR', '首尔专线'],
      [`${FLAG_AE} 迪拜节点`, 'AE', '迪拜节点'],
      [`${FLAG_CA} 多伦多`, 'CA', '多伦多'],
      [`${FLAG_AU} 悉尼专线`, 'AU', '悉尼专线'],
      [`${FLAG_NL} 阿姆斯特丹`, 'NL', '阿姆斯特丹'],
    ]

    for (const [input, expectedCode, expectedName] of cases) {
      expect(cleanNodeDisplay(input)).toEqual({ code: expectedCode, name: expectedName })
    }
  })

  it('strips leading pictographs without inventing country codes and falls back if empty', () => {
    expect(cleanNodeDisplay(`${PICTO_GLOBE} 默认代理`)).toEqual({ code: null, name: '默认代理' })
    expect(cleanNodeDisplay(`${PICTO_LOCK} 手动节点`)).toEqual({ code: null, name: '手动节点' })
    expect(cleanNodeDisplay(`${PICTO_BOLT} 自动优选`)).toEqual({ code: null, name: '自动优选' })
    expect(cleanNodeDisplay(`${PICTO_STOP} 已阻断`)).toEqual({ code: null, name: '已阻断' })
    expect(cleanNodeDisplay('vps-34-97-90-168')).toEqual({ code: null, name: 'vps-34-97-90-168' })
    expect(cleanNodeDisplay(`  ${PICTO_GLOBE}  `)).toEqual({ code: null, name: PICTO_GLOBE })
    expect(cleanNodeDisplay('   ')).toEqual({ code: null, name: '' })
  })
})

describe('SelectPopover', () => {
  const baseOptions: SelectOption[] = [
    { value: 'pool', label: '默认代理', tone: 'cobalt' },
    { value: 'block_overseas', label: '禁止走代理', tone: 'amber' },
    { value: 'direct', label: '全量直连', tone: 'default' },
    {
      value: `dedicated:${FLAG_US} 洛杉矶节点`,
      label: `${FLAG_US} 洛杉矶节点`,
      group: '手动节点',
      badge: '42 ms',
      tone: 'emerald',
    },
    {
      value: 'dedicated:offline-line',
      label: '离线节点',
      group: '手动节点',
      badge: '离线',
      tone: 'crimson',
      disabled: true,
    },
  ]

  it('renders custom listbox without native form dropdowns and preserves raw option value on selection', async () => {
    const user = userEvent.setup()
    const [val, setVal] = createSignal('pool')
    const onChange = rs.fn((next: string) => setVal(next))

    const { container } = render(() => (
      <SelectPopover
        value={val()}
        options={baseOptions}
        onChange={onChange}
        ariaLabel="设置设备出口策略"
        fullWidth
      />
    ))

    expect(container.querySelectorAll('select')).toHaveLength(0)
    expect(container.querySelector('.picker-root.full-width')).toBeInTheDocument()

    const trigger = screen.getByRole('button', { name: '设置设备出口策略' })
    expect(trigger).toHaveAttribute('aria-expanded', 'false')
    expect(trigger).toHaveTextContent('默认代理')

    await user.click(trigger)
    expect(trigger).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('listbox', { name: '设置设备出口策略' })).toBeInTheDocument()
    expect(container.querySelector('.picker-search')).not.toBeInTheDocument()
    expect(screen.getByText('手动节点')).toHaveClass('picker-group-label')

    const usOption = screen.getByRole('option', { name: /洛杉矶节点/ })
    expect(usOption).toHaveTextContent('US')
    expect(usOption).toHaveTextContent('42 ms')

    await user.click(usOption)
    expect(onChange).toHaveBeenCalledWith(`dedicated:${FLAG_US} 洛杉矶节点`)
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()
    expect(trigger).toHaveTextContent('US')
    expect(trigger).toHaveTextContent('洛杉矶节点')
  })

  it('shows instant search above threshold and filters options by keyword or ISO code', async () => {
    const user = userEvent.setup()
    const manyOptions: SelectOption[] = [
      { value: 'n1', label: `${FLAG_HK} 香港专线 01`, badge: '18 ms' },
      { value: 'n2', label: `${FLAG_HK} 香港专线 02`, badge: '21 ms' },
      { value: 'n3', label: `${FLAG_JP} 东京高速 01`, badge: '38 ms' },
      { value: 'n4', label: `${FLAG_JP} 大阪高速 02`, badge: '44 ms' },
      { value: 'n5', label: `${FLAG_SG} 新加坡 BGP 01`, badge: '59 ms' },
      { value: 'n6', label: `${FLAG_US} 洛杉矶原生 01`, badge: '132 ms' },
      { value: 'n7', label: `${FLAG_DE} 法兰克福 01`, badge: '168 ms' },
    ]
    const onChange = rs.fn()

    render(() => (
      <SelectPopover
        value="n1"
        options={manyOptions}
        onChange={onChange}
        ariaLabel="切换公共池主出口"
        size="sm"
        align="right"
      />
    ))

    await user.click(screen.getByRole('button', { name: '切换公共池主出口' }))
    const listbox = screen.getByRole('listbox')
    expect(listbox).toHaveClass('align-right')

    const searchInput = screen.getByRole('textbox')
    await user.type(searchInput, 'JP')
    const filtered = screen.getAllByRole('option')
    expect(filtered).toHaveLength(2)
    expect(filtered[0]).toHaveTextContent('东京高速 01')
    expect(filtered[1]).toHaveTextContent('大阪高速 02')

    await user.clear(searchInput)
    await user.type(searchInput, '不存在的节点')
    expect(screen.queryAllByRole('option')).toHaveLength(0)
    expect(screen.getByText('无匹配选项')).toBeInTheDocument()
  })

  it('supports keyboard navigation, skips disabled items, and closes on Escape or outside pointerdown', async () => {
    const user = userEvent.setup()
    const onChange = rs.fn()

    render(() => (
      <div>
        <button type="button">外部区域</button>
        <SelectPopover
          value="pool"
          options={baseOptions}
          onChange={onChange}
          ariaLabel="键盘选择测试"
        />
      </div>
    ))

    const trigger = screen.getByRole('button', { name: '键盘选择测试' })
    trigger.focus()

    await user.keyboard('{ArrowDown}')
    expect(screen.getByRole('listbox')).toBeInTheDocument()

    await user.keyboard('{ArrowDown}{Enter}')
    expect(onChange).toHaveBeenCalledWith('block_overseas')
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()

    await user.click(trigger)
    expect(screen.getByRole('listbox')).toBeInTheDocument()
    await user.keyboard('{ArrowUp}{Enter}')
    expect(onChange).toHaveBeenLastCalledWith(`dedicated:${FLAG_US} 洛杉矶节点`)
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()

    await user.click(trigger)
    expect(screen.getByRole('listbox')).toBeInTheDocument()
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()

    await user.click(trigger)
    expect(screen.getByRole('listbox')).toBeInTheDocument()
    fireEvent.pointerDown(screen.getByRole('button', { name: '外部区域' }))
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()
  })
})
