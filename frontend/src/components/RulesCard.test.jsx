import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { RulesCard } from './RulesCard.jsx'

const rules = [
  { index: 0, field: 'process_name', value: 'curl', target: 'direct', raw: '{"process_name":"curl","action":"route","outbound":"direct"}' },
  { index: 1, field: 'port', value: '25', target: 'reject', raw: '{"port":25,"action":"reject"}' },
  { index: 2, raw: '{"rule_set":["custom"],"action":"route","outbound":"proxy"}' },
]

function renderCard(props = {}) {
  return render(
    <RulesCard
      rules={rules}
      isInitializing={false}
      loadingAction=""
      onAddRule={vi.fn().mockResolvedValue(true)}
      onDeleteRule={vi.fn()}
      adblockEnabled={false}
      onToggleAdblock={vi.fn()}
      {...props}
    />,
  )
}

describe('RulesCard', () => {
  it('renders structured rules with labels and target badges', () => {
    renderCard()

    expect(screen.getByText('自定义规则')).toBeInTheDocument()
    expect(screen.getByText('curl')).toBeInTheDocument()
    // 字段标签同时出现在行 chip 与添加区的下拉选项中
    expect(screen.getAllByText('进程名').length).toBeGreaterThan(0)
    expect(screen.getAllByText('直连').length).toBeGreaterThan(0)
    expect(screen.getByText('25')).toBeInTheDocument()
    expect(screen.getAllByText('拦截').length).toBeGreaterThan(0)
    // 手写 JSON 规则以原文兜底展示
    expect(screen.getByText('{"rule_set":["custom"],"action":"route","outbound":"proxy"}')).toBeInTheDocument()
    expect(screen.getByText('自定义 JSON 规则')).toBeInTheDocument()
  })

  it('adds a rule with the selected field, value, and target', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(true)
    renderCard({ onAddRule })

    await user.selectOptions(screen.getByLabelText('规则字段'), 'process_name')
    await user.selectOptions(screen.getByLabelText('规则目标'), 'direct')
    const input = screen.getByLabelText('规则值')
    await user.type(input, 'curl')
    await user.click(screen.getByRole('button', { name: '添加' }))

    expect(onAddRule).toHaveBeenCalledWith({ field: 'process_name', value: 'curl', target: 'direct' })
    // 添加成功后清空输入框
    expect(input).toHaveValue('')
  })

  it('keeps the input when adding fails', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(false)
    renderCard({ onAddRule })

    const input = screen.getByLabelText('规则值')
    await user.type(input, 'example.com')
    await user.click(screen.getByRole('button', { name: '添加' }))

    expect(onAddRule).toHaveBeenCalled()
    expect(input).toHaveValue('example.com')
  })

  it('deletes a rule by its index and raw payload', async () => {
    const user = userEvent.setup()
    const onDeleteRule = vi.fn()
    renderCard({ onDeleteRule })

    await user.click(screen.getByRole('button', { name: '删除规则 curl' }))
    expect(onDeleteRule).toHaveBeenCalledWith(rules[0])
  })

  it('disables adding when the value is empty', () => {
    renderCard()
    expect(screen.getByRole('button', { name: '添加' })).toBeDisabled()
  })

  it('shows an empty state when there are no rules', () => {
    renderCard({ rules: [] })
    expect(screen.getByText('暂无自定义规则')).toBeInTheDocument()
  })

  it('renders the adblock switch next to the title and toggles on click', async () => {
    const user = userEvent.setup()
    const onToggleAdblock = vi.fn()
    renderCard({ adblockEnabled: false, onToggleAdblock })

    const toggle = screen.getByRole('switch', { name: '去广告' })
    expect(toggle).toHaveAttribute('aria-checked', 'false')

    await user.click(toggle)
    expect(onToggleAdblock).toHaveBeenCalledWith(true)
  })

  it('reflects the enabled adblock state and disables the switch while pending', () => {
    renderCard({ adblockEnabled: true, loadingAction: 'toggleAdblock' })

    const toggle = screen.getByRole('switch', { name: '去广告' })
    expect(toggle).toHaveAttribute('aria-checked', 'true')
    expect(toggle).toBeDisabled()
  })
})
