import { act, fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { RulesCard } from './RulesCard'
import { ruleMock } from '../testFixtures'

const rules = [
  ruleMock({ index: 0, field: 'process_name', value: 'curl', target: 'direct', raw: '{"process_name":"curl","action":"route","outbound":"direct"}' }),
  ruleMock({ index: 1, field: 'port', value: '25', target: 'reject', raw: '{"port":25,"action":"reject"}' }),
  ruleMock({ index: 2, raw: '{"rule_set":["custom"],"action":"route","outbound":"proxy"}' }),
]

function renderCard(props = {}) {
  return render(
    <RulesCard
      rules={rules}
      isInitializing={false}
      pendingActions={new Set()}
      onAddRule={rs.fn().mockResolvedValue(true)}
      onDeleteRule={rs.fn()}
      {...props}
    />,
  )
}

async function openRuleModal(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: '添加' }))
  return screen.getByRole('dialog')
}

async function openNodeTargets(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: '或指定一个节点' }))
  return screen.getByRole('radiogroup', { name: '指定节点' })
}

describe('RulesCard', () => {
  it('locks the submitted draft, prevents duplicate Enter requests, and leaves a reopened dialog open', async () => {
    const user = userEvent.setup()
    let finish!: (added: boolean) => void
    const onAddRule = rs.fn(() => new Promise<boolean>(resolve => { finish = resolve }))
    renderCard({ onAddRule })
    await openRuleModal(user)
    await user.type(screen.getByLabelText('规则值'), 'example.com')
    await user.click(screen.getByRole('radio', { name: /^直接连接/ }))
    act(() => {
      fireEvent.keyDown(screen.getByLabelText('规则值'), { key: 'Enter' })
      fireEvent.keyDown(screen.getByLabelText('规则值'), { key: 'Enter' })
    })
    expect(onAddRule).toHaveBeenCalledExactlyOnceWith({ field: 'domain_suffix', value: 'example.com', target: 'direct' })
    expect(screen.getByLabelText('规则值')).toBeDisabled()
    expect(screen.getByLabelText('匹配方式')).toBeDisabled()
    expect(screen.getByRole('radio', { name: /^走代理/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'google.com' })).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent('关闭窗口不会取消请求')
    await user.click(screen.getByRole('button', { name: '关闭窗口' }))
    await openRuleModal(user)
    await act(async () => { finish(true) })
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(screen.getByLabelText('规则值')).toHaveValue('')
    expect(screen.getByLabelText('规则值')).toBeEnabled()
  })

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
    // 默认选中网站 + 代理
    expect(screen.getByRole('radio', { name: /网站/ })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('radio', { name: /^走代理/ })).toHaveAttribute('aria-checked', 'true')
  })

  it('adds a rule through the modal form and closes on success', async () => {
    const user = userEvent.setup()
    const onAddRule = rs.fn().mockResolvedValue(true)
    renderCard({ onAddRule })

    await openRuleModal(user)
    await user.click(screen.getByRole('radio', { name: /应用/ }))
    await user.click(screen.getByRole('radio', { name: /^直接连接/ }))
    await user.type(screen.getByLabelText('规则值'), 'curl')
    await user.click(screen.getByRole('button', { name: '添加规则' }))

    expect(onAddRule).toHaveBeenCalledWith({ field: 'process_name', value: 'curl', target: 'direct' })
    // 添加成功后弹窗关闭
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('fills the value from common app chips for process fields', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    await user.click(screen.getByRole('radio', { name: /应用/ }))
    await user.click(screen.getByRole('button', { name: 'qBittorrent' }))

    expect(screen.getByLabelText('规则值')).toHaveValue('qbittorrent')
  })


  it('fills the value from common site chips for domain fields', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    await user.click(screen.getByRole('button', { name: 'openai.com' }))

    expect(screen.getByLabelText('规则值')).toHaveValue('openai.com')
  })

  it('previews the rule in plain language without exposing JSON', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    expect(screen.getByText(/填写上方内容后/)).toBeInTheDocument()

    await user.type(screen.getByLabelText('规则值'), 'example.com')

    expect(screen.getByText('访问 example.com 及其子域名时，走代理。')).toBeInTheDocument()
    expect(screen.queryByText('{"domain_suffix":"example.com","action":"route","outbound":"proxy"}')).not.toBeInTheDocument()
  })

  it('keeps the modal input when adding fails', async () => {
    const user = userEvent.setup()
    const onAddRule = rs.fn().mockResolvedValue(false)
    renderCard({ onAddRule })

    await openRuleModal(user)
    const input = screen.getByLabelText('规则值')
    await user.type(input, 'example.com')
    await user.click(screen.getByRole('button', { name: '添加规则' }))

    expect(onAddRule).toHaveBeenCalled()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(input).toHaveValue('example.com')
    expect(input).toBeEnabled()
    expect(screen.getByRole('button', { name: '添加规则' })).toBeEnabled()
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

  it('explains invalid website input before submission', async () => {
    const user = userEvent.setup()
    const onAddRule = rs.fn().mockResolvedValue(true)
    renderCard({ onAddRule })

    await openRuleModal(user)
    await user.type(screen.getByLabelText('规则值'), 'https://example.com/path')

    expect(screen.getByRole('alert')).toHaveTextContent('只填写域名或关键词')
    expect(screen.getByRole('button', { name: '添加规则' })).toBeDisabled()
    expect(onAddRule).not.toHaveBeenCalled()

    await user.clear(screen.getByLabelText('规则值'))
    await user.type(screen.getByLabelText('规则值'), 'example.com')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '添加规则' })).toBeEnabled()
  })

  it('uses a select with a default value for the protocol field', async () => {
    const user = userEvent.setup()
    const onAddRule = rs.fn().mockResolvedValue(true)
    renderCard({ onAddRule })

    await openRuleModal(user)
    await user.click(screen.getByRole('radio', { name: /更多方式/ }))
    await user.selectOptions(screen.getByLabelText('匹配方式'), 'protocol')

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
    await user.click(screen.getByRole('radio', { name: /更多方式/ }))
    await user.selectOptions(screen.getByLabelText('匹配方式'), 'protocol')
    await user.click(screen.getByRole('radio', { name: /网站/ }))

    const valueControl = screen.getByLabelText('规则值')
    expect(valueControl.tagName).toBe('INPUT')
    expect(valueControl).toHaveValue('')
  })

  it('keeps a separate draft per field instead of carrying values across types', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    await user.type(screen.getByLabelText('规则值'), 'netflix.com')

    // 切到端口范围：域名的值不会被带过来（此前会把 netflix.com 留在端口范围下）
    await user.click(screen.getByRole('radio', { name: /更多方式/ }))
    await user.selectOptions(screen.getByLabelText('匹配方式'), 'port_range')
    expect(screen.getByLabelText('规则值')).toHaveValue('')

    // 切回域名后缀：草稿还在
    await user.click(screen.getByRole('radio', { name: /网站/ }))
    expect(screen.getByLabelText('规则值')).toHaveValue('netflix.com')
  })

  it('deletes a rule by its index and raw payload', async () => {
    const user = userEvent.setup()
    const onDeleteRule = rs.fn()
    renderCard({ onDeleteRule })

    await user.click(screen.getByRole('button', { name: '删除规则 curl' }))
    expect(onDeleteRule).toHaveBeenCalledWith(rules[0])
  })

  it('shows an empty state when there are no rules', () => {
    renderCard({ rules: [] })
    expect(screen.getByText('暂无自定义规则')).toBeInTheDocument()
  })

  it('offers node names as rule targets and submits the selected node', async () => {
    const user = userEvent.setup()
    const onAddRule = rs.fn().mockResolvedValue(true)
    renderCard({ onAddRule, nodeNames: ['香港节点'] })

    await openRuleModal(user)
    await openNodeTargets(user)
    await user.click(screen.getByRole('radio', { name: /香港节点/ }))
    // 选中节点目标后提示节点失效风险
    expect(screen.getByText(/出口节点消失或被禁用后，匹配此规则的流量将被阻断/)).toBeInTheDocument()

    await user.type(screen.getByLabelText('规则值'), 'example.com')
    await user.click(screen.getByRole('button', { name: '添加规则' }))
    expect(onAddRule).toHaveBeenCalledWith({ field: 'domain_suffix', value: 'example.com', target: '香港节点' })
  })

  it('filters node targets by the search box', async () => {
    const user = userEvent.setup()
    renderCard({ nodeNames: ['香港节点', '新加坡节点', '美国节点'] })

    await openRuleModal(user)
    const nodeList = within(await openNodeTargets(user))
    expect(nodeList.getAllByRole('radio')).toHaveLength(3)

    await user.type(screen.getByLabelText('搜索节点'), '香港')
    expect(screen.getByRole('radio', { name: /香港节点/ })).toBeInTheDocument()
    expect(screen.queryByRole('radio', { name: /新加坡节点/ })).not.toBeInTheDocument()
  })

  it('keeps node targets collapsed and tests candidates only when expanded', async () => {
    const user = userEvent.setup()
    const onTestNodes = rs.fn()
    renderCard({
      nodeNames: ['香港节点', '新加坡节点', '美国节点'],
      onTestNodes,
      delays: { '香港节点': 132, '新加坡节点': -1 },
    })

    await openRuleModal(user)
    expect(screen.getByRole('button', { name: '或指定一个节点' })).toHaveAttribute('aria-expanded', 'false')
    expect(onTestNodes).not.toHaveBeenCalled()

    await openNodeTargets(user)
    expect(onTestNodes).toHaveBeenCalledTimes(1)

    // 同一次弹窗会话里重新折叠并展开，不重复启动一整批测速
    const nodeTargetsToggle = screen.getByRole('button', { name: '或指定一个节点' })
    await user.click(nodeTargetsToggle)
    await user.click(nodeTargetsToggle)
    expect(onTestNodes).toHaveBeenCalledTimes(1)

    expect(screen.getByRole('radio', { name: /香港节点/ }).textContent).toContain('132 ms')
    expect(screen.getByRole('radio', { name: /新加坡节点/ }).textContent).toContain('超时')
  })

  it('does not start another node test batch while a candidate is already testing', async () => {
    const user = userEvent.setup()
    const onTestNodes = rs.fn()
    renderCard({
      nodeNames: ['香港节点', '美国节点'],
      onTestNodes,
      testingNodes: { '美国节点': true },
    })

    await openRuleModal(user)
    await openNodeTargets(user)

    expect(onTestNodes).not.toHaveBeenCalled()
    expect(screen.getByRole('radio', { name: /美国节点/ }).textContent).toContain('测速中')
  })

  it('renders node-target rules with a neutral node badge', () => {
    renderCard({
      rules: [
        { index: 0, field: 'process_name', value: 'curl', target: '香港节点', raw: '{"process_name":"curl","action":"route","outbound":"香港节点"}' },
      ],
    })

    const badge = screen.getByText('香港节点').closest('.rule-target-badge')
    expect(badge).not.toBeNull()
    expect(badge).toHaveClass('rule-target-badge', 'node')
  })

  it('pulses a rule row when a live connection reports that rule', () => {
    renderCard({
      connections: [
        { id: 'c1', rule: 'process_name=curl => route(direct)' },
        { id: 'c2', rule: 'final' },
      ],
    })

    const activeRow = screen.getByText('curl').closest('.list-row')!
    const inactiveRow = screen.getByText('25').closest('.list-row')!
    expect(activeRow).toHaveClass('rule-active')
    expect(activeRow).toHaveAttribute('title', '该规则正在匹配连接')
    expect(activeRow.querySelector('.rule-status-slot .rule-live-dot')).toBeInTheDocument()
    expect(inactiveRow).not.toHaveClass('rule-active')
    expect(inactiveRow.querySelector('.rule-status-slot')).toBeEmptyDOMElement()
  })

  it('marks missing outbound rules as blocking traffic', () => {
    renderCard({
      rules: [
        { index: 0, field: 'process_name', value: 'nginx', target: 'ghost-node', skipped: true, raw: '{"process_name":"nginx","action":"route","outbound":"ghost-node"}' },
        { index: 1, field: 'domain', value: 't.co', target: 'proxy', raw: '{"domain":"t.co","action":"route","outbound":"proxy"}' },
      ],
    })

    const icon = screen.getByLabelText('出口缺失，已阻断')
    expect(icon).toHaveAttribute('title', expect.stringContaining('匹配此规则的流量已阻断'))
    expect(icon.closest('.list-row')).toHaveClass('blocked')
    expect(icon.parentElement).toHaveClass('rule-status-slot')
    // 正常规则不带出口缺失标记
    expect(screen.getByText('t.co').closest('.list-row')).not.toHaveClass('blocked')
  })
})
