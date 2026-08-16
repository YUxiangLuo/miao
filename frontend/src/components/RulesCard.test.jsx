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

async function openRuleModal(user) {
  await user.click(screen.getByRole('button', { name: '添加' }))
  return screen.getByRole('dialog')
}

describe('RulesCard', () => {
  it('renders structured rules with labels and target badges', () => {
    renderCard()

    expect(screen.getByText('自定义规则')).toBeInTheDocument()
    expect(screen.getByText('curl')).toBeInTheDocument()
    expect(screen.getAllByText('进程名').length).toBeGreaterThan(0)
    expect(screen.getAllByText('直连').length).toBeGreaterThan(0)
    expect(screen.getByText('25')).toBeInTheDocument()
    expect(screen.getAllByText('拦截').length).toBeGreaterThan(0)
    // 手写 JSON 规则以原文兜底展示
    expect(screen.getByText('{"rule_set":["custom"],"action":"route","outbound":"proxy"}')).toBeInTheDocument()
    expect(screen.getByText('自定义 JSON 规则')).toBeInTheDocument()
  })

  it('keeps the rule modal closed until the add button is clicked', async () => {
    const user = userEvent.setup()
    renderCard()

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()

    const dialog = await openRuleModal(user)
    expect(dialog).toBeInTheDocument()
  })

  it('adds a rule through the modal form and closes on success', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(true)
    renderCard({ onAddRule })

    await openRuleModal(user)
    await user.selectOptions(screen.getByLabelText('规则字段'), 'process_name')
    await user.selectOptions(screen.getByLabelText('规则目标'), 'direct')
    await user.type(screen.getByLabelText('规则值'), 'curl')
    await user.click(screen.getByRole('button', { name: '添加规则' }))

    expect(onAddRule).toHaveBeenCalledWith({ field: 'process_name', value: 'curl', target: 'direct' })
    // 添加成功后弹窗关闭
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('keeps the modal input when adding fails', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(false)
    renderCard({ onAddRule })

    await openRuleModal(user)
    const input = screen.getByLabelText('规则值')
    await user.type(input, 'example.com')
    await user.click(screen.getByRole('button', { name: '添加规则' }))

    expect(onAddRule).toHaveBeenCalled()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(input).toHaveValue('example.com')
  })

  it('resets the modal form after it is closed and reopened', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    await user.type(screen.getByLabelText('规则值'), 'example.com')
    await user.click(screen.getByRole('button', { name: '取消' }))
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()

    await openRuleModal(user)
    expect(screen.getByLabelText('规则值')).toHaveValue('')
  })

  it('disables submitting when the value is empty', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    expect(screen.getByRole('button', { name: '添加规则' })).toBeDisabled()
  })

  it('uses a select with a default value for the protocol field', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(true)
    renderCard({ onAddRule })

    await openRuleModal(user)
    await user.selectOptions(screen.getByLabelText('规则字段'), 'protocol')

    const valueControl = screen.getByLabelText('规则值')
    expect(valueControl.tagName).toBe('SELECT')
    expect(valueControl).toHaveValue('quic')

    await user.click(screen.getByRole('button', { name: '添加规则' }))
    expect(onAddRule).toHaveBeenCalledWith({ field: 'protocol', value: 'quic', target: 'proxy' })
  })

  it('clears the protocol value when switching back to a text field', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    await user.selectOptions(screen.getByLabelText('规则字段'), 'protocol')
    await user.selectOptions(screen.getByLabelText('规则字段'), 'domain_suffix')

    const valueControl = screen.getByLabelText('规则值')
    expect(valueControl.tagName).toBe('INPUT')
    expect(valueControl).toHaveValue('')
  })

  it('deletes a rule by its index and raw payload', async () => {
    const user = userEvent.setup()
    const onDeleteRule = vi.fn()
    renderCard({ onDeleteRule })

    await user.click(screen.getByRole('button', { name: '删除规则 curl' }))
    expect(onDeleteRule).toHaveBeenCalledWith(rules[0])
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

  it('offers node names as rule targets and submits the selected node', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(true)
    renderCard({ onAddRule, nodeNames: ['香港节点'] })

    await openRuleModal(user)
    await user.selectOptions(screen.getByLabelText('规则目标'), '香港节点')
    // 选中节点目标后提示节点失效风险
    expect(screen.getByText(/节点日后消失/)).toBeInTheDocument()

    await user.type(screen.getByLabelText('规则值'), 'example.com')
    await user.click(screen.getByRole('button', { name: '添加规则' }))
    expect(onAddRule).toHaveBeenCalledWith({ field: 'domain_suffix', value: 'example.com', target: '香港节点' })
  })

  it('tests all candidate nodes on modal open and shows delays in the options', async () => {
    const user = userEvent.setup()
    const onTestNodes = vi.fn()
    renderCard({
      nodeNames: ['香港节点', '新加坡节点', '美国节点'],
      onTestNodes,
      delays: { '香港节点': 132, '新加坡节点': -1 },
      testingNodes: { '美国节点': true },
    })

    await openRuleModal(user)
    expect(onTestNodes).toHaveBeenCalledTimes(1)

    expect(screen.getByRole('option', { name: /香港节点/ }).textContent).toContain('132 ms')
    expect(screen.getByRole('option', { name: /新加坡节点/ }).textContent).toContain('超时')
    expect(screen.getByRole('option', { name: /美国节点/ }).textContent).toContain('测速中')
  })

  it('renders node-target rules with a neutral node badge', () => {
    renderCard({
      rules: [
        { index: 0, field: 'process_name', value: 'curl', target: '香港节点', raw: '{"process_name":"curl","action":"route","outbound":"香港节点"}' },
      ],
    })

    const badge = screen.getByText('香港节点')
    expect(badge).toHaveClass('rule-target-badge', 'node')
  })

  it('pulses a rule row when a live connection reports that rule', () => {
    renderCard({
      connections: [
        { id: 'c1', rule: 'process_name=curl => route(direct)' },
        { id: 'c2', rule: 'final' },
      ],
    })

    expect(screen.getByText('curl').closest('.list-row')).toHaveClass('rule-active')
    expect(screen.getByText('curl').closest('.list-row')).toHaveAttribute('title', '该规则正在匹配连接')
    expect(screen.getByText('25').closest('.list-row')).not.toHaveClass('rule-active')
  })

  it('marks skipped rules with a warning icon and dims the row', () => {
    renderCard({
      rules: [
        { index: 0, field: 'process_name', value: 'nginx', target: 'ghost-node', skipped: true, raw: '{"process_name":"nginx","action":"route","outbound":"ghost-node"}' },
        { index: 1, field: 'domain', value: 't.co', target: 'proxy', raw: '{"domain":"t.co","action":"route","outbound":"proxy"}' },
      ],
    })

    const icon = screen.getByLabelText('规则未生效')
    expect(icon).toHaveAttribute('title', expect.stringContaining('未生效'))
    expect(icon.closest('.list-row')).toHaveClass('skipped')
    // 正常规则不带失效标记
    expect(screen.getByText('t.co').closest('.list-row')).not.toHaveClass('skipped')
  })
})
