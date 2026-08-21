import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
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
      loadingAction=""
      onAddRule={vi.fn().mockResolvedValue(true)}
      onDeleteRule={vi.fn()}
      {...props}
    />,
  )
}

async function openRuleModal(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: '添加' }))
  return screen.getByRole('dialog')
}

async function openNodeTargets(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: '或指定节点出口' }))
  return screen.getByRole('radiogroup', { name: '指定节点' })
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
    // 默认选中 域名后缀 + 代理
    expect(screen.getByRole('radio', { name: /域名后缀/ })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('radio', { name: /^代理/ })).toHaveAttribute('aria-checked', 'true')
  })

  it('adds a rule through the modal form and closes on success', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(true)
    renderCard({ onAddRule })

    await openRuleModal(user)
    await user.click(screen.getByRole('radio', { name: /进程名/ }))
    await user.click(screen.getByRole('radio', { name: /^直连/ }))
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
    await user.click(screen.getByRole('radio', { name: /进程名/ }))
    await user.click(screen.getByRole('button', { name: 'qBittorrent' }))

    expect(screen.getByLabelText('规则值')).toHaveValue('qbittorrent')
  })

  it('uses .exe names for app chips on windows', async () => {
    const user = userEvent.setup()
    renderCard({ platform: 'windows' })

    await openRuleModal(user)
    await user.click(screen.getByRole('radio', { name: /进程名/ }))
    await user.click(screen.getByRole('button', { name: 'qBittorrent' }))

    expect(screen.getByLabelText('规则值')).toHaveValue('qbittorrent.exe')
  })

  it('fills the value from common site chips for domain fields', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    await user.click(screen.getByRole('button', { name: 'openai.com' }))

    expect(screen.getByLabelText('规则值')).toHaveValue('openai.com')
  })

  it('previews the rule in plain language and as stored JSON', async () => {
    const user = userEvent.setup()
    renderCard()

    await openRuleModal(user)
    expect(screen.getByText(/填写匹配值后/)).toBeInTheDocument()

    await user.type(screen.getByLabelText('规则值'), 'example.com')

    expect(screen.getByText('凡是 域名以 example.com 结尾的站点 的连接 → 走代理')).toBeInTheDocument()
    expect(screen.getByText('{"domain_suffix":"example.com","action":"route","outbound":"proxy"}')).toBeInTheDocument()
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
    await user.click(screen.getByRole('radio', { name: /嗅探协议/ }))

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
    await user.click(screen.getByRole('radio', { name: /嗅探协议/ }))
    await user.click(screen.getByRole('radio', { name: /域名后缀/ }))

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
    await user.click(screen.getByRole('radio', { name: /端口范围/ }))
    expect(screen.getByLabelText('规则值')).toHaveValue('')

    // 切回域名后缀：草稿还在
    await user.click(screen.getByRole('radio', { name: /域名后缀/ }))
    expect(screen.getByLabelText('规则值')).toHaveValue('netflix.com')
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

  it('offers node names as rule targets and submits the selected node', async () => {
    const user = userEvent.setup()
    const onAddRule = vi.fn().mockResolvedValue(true)
    renderCard({ onAddRule, nodeNames: ['香港节点'] })

    await openRuleModal(user)
    await openNodeTargets(user)
    await user.click(screen.getByRole('radio', { name: /香港节点/ }))
    // 选中节点目标后提示节点失效风险
    expect(screen.getByText(/节点日后消失/)).toBeInTheDocument()

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
    const onTestNodes = vi.fn()
    renderCard({
      nodeNames: ['香港节点', '新加坡节点', '美国节点'],
      onTestNodes,
      delays: { '香港节点': 132, '新加坡节点': -1 },
    })

    await openRuleModal(user)
    expect(screen.getByRole('button', { name: '或指定节点出口' })).toHaveAttribute('aria-expanded', 'false')
    expect(onTestNodes).not.toHaveBeenCalled()

    await openNodeTargets(user)
    expect(onTestNodes).toHaveBeenCalledTimes(1)

    // 同一次弹窗会话里重新折叠并展开，不重复启动一整批测速
    const nodeTargetsToggle = screen.getByRole('button', { name: '或指定节点出口' })
    await user.click(nodeTargetsToggle)
    await user.click(nodeTargetsToggle)
    expect(onTestNodes).toHaveBeenCalledTimes(1)

    expect(screen.getByRole('radio', { name: /香港节点/ }).textContent).toContain('132 ms')
    expect(screen.getByRole('radio', { name: /新加坡节点/ }).textContent).toContain('超时')
  })

  it('does not start another node test batch while a candidate is already testing', async () => {
    const user = userEvent.setup()
    const onTestNodes = vi.fn()
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
    expect(icon.parentElement).toHaveClass('rule-status-slot')
    // 正常规则不带失效标记
    expect(screen.getByText('t.co').closest('.list-row')).not.toHaveClass('skipped')
  })
})
