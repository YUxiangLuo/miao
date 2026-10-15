import { useEffect, useId, useRef, useState } from 'react'
import { Ban, ChevronDown, Globe, ListFilter, Plus, Search, TriangleAlert, X, Zap } from 'lucide-react'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { Button } from './ui'
import { PROTOCOL_OPTIONS, RULE_FIELD_OPTIONS } from '../ruleFormat'
import { COMMON_DOMAIN_SITES, COMMON_PROCESS_APPS } from '../ruleApps'
import { rulePlainPreview } from '../rulePreview'
import { classNames } from '../utils'
import type { RuleRequest } from '../types/api'

const MATCH_GROUPS = [
  { id: 'website', label: '网站', description: '按网站地址匹配', fields: ['domain_suffix', 'domain', 'domain_keyword'] },
  { id: 'app', label: '应用', description: '按电脑上的应用匹配', fields: ['process_name', 'process_path'] },
  { id: 'advanced', label: '更多方式', description: 'IP、端口或协议', fields: ['ip_cidr', 'source_ip_cidr', 'port', 'port_range', 'protocol'] },
] as const

const MATCH_LABELS: Record<string, string> = {
  domain_suffix: '整个网站（含子域名）', domain: '仅这个域名', domain_keyword: '域名包含关键词',
  process_name: '应用名称', process_path: '应用完整路径',
  ip_cidr: '目标 IP 地址段', source_ip_cidr: '来源 IP 地址段',
  port: '目标端口', port_range: '目标端口范围', protocol: '网络协议',
}

const VALUE_LABELS: Record<string, string> = {
  domain_suffix: '网站地址', domain: '域名', domain_keyword: '域名关键词',
  process_name: '应用名称', process_path: '应用路径',
  ip_cidr: '目标 IP 地址段', source_ip_cidr: '来源 IP 地址段',
  port: '端口号', port_range: '端口范围', protocol: '网络协议',
}

const VALUE_HELP: Record<string, string> = {
  domain_suffix: '例如 example.com，也会匹配 www.example.com；只填域名即可。',
  domain: '例如 www.example.com，只匹配这个完整域名。',
  domain_keyword: '例如 google，会匹配域名中包含 google 的网站。',
  process_name: '填写应用的可执行文件名，或从下方直接选择。',
  process_path: '填写应用可执行文件的完整路径。',
  ip_cidr: '例如 192.168.0.0/16。', source_ip_cidr: '例如 192.168.1.0/24。',
  port: '填写 1–65535 之间的端口号。', port_range: '例如 1000:2000。',
  protocol: '选择连接使用的协议。',
}

const TARGET_OPTIONS = [
  { value: 'proxy', label: '走代理', description: '使用当前代理出口', icon: Globe },
  { value: 'direct', label: '直接连接', description: '绕过代理访问', icon: Zap },
  { value: 'reject', label: '阻止连接', description: '不允许访问', icon: Ban },
]

const PROCESS_EXAMPLES = [
  COMMON_PROCESS_APPS[0].apps[0], COMMON_PROCESS_APPS[1].apps[0],
  COMMON_PROCESS_APPS[2].apps[0], COMMON_PROCESS_APPS[4].apps[0],
]

function valueError(field: string, value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  if ([...trimmed].length > 256) return '最多填写 256 个字符'
  if (field.startsWith('domain') && /[/:\s]/.test(trimmed)) return '只填写域名或关键词，不要包含 https://、路径或空格'
  if (field === 'port' && (!/^\d+$/.test(trimmed) || Number(trimmed) < 1 || Number(trimmed) > 65535)) return '端口号需在 1–65535 之间'
  if (field === 'port_range') {
    const match = /^(\d*):(\d*)$/.exec(trimmed)
    const start = Number(match?.[1])
    const end = Number(match?.[2])
    if (!match || (!match[1] && !match[2]) || (match[1] && (start < 1 || start > 65535)) || (match[2] && (end < 1 || end > 65535)) || (match[1] && match[2] && start > end)) {
      return '请填写有效范围，例如 1000:2000'
    }
  }
  return null
}

export interface RuleModalProps {
  open: boolean
  loading: boolean
  onClose: () => void
  onSubmit: (rule: RuleRequest) => Promise<boolean>
  nodeNames?: string[]
  delays?: Record<string, number>
  testingNodes?: Record<string, boolean>
  onTestNodes?: () => void
}

export function RuleModal({ open, loading, onClose, onSubmit, nodeNames = [], delays = {}, testingNodes = {}, onTestNodes }: RuleModalProps) {
  const titleId = useId()
  const valueId = useId()
  const errorId = useId()
  const nodeTargetsId = useId()
  const dialogRef = useDialog(open, onClose)
  const [field, setField] = useState('domain_suffix')
  const [target, setTarget] = useState('proxy')
  const [value, setValue] = useState('')
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [nodeQuery, setNodeQuery] = useState('')
  const [nodeTargetsOpen, setNodeTargetsOpen] = useState(false)
  const [nodeTestStarted, setNodeTestStarted] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const inFlight = useRef(false)
  const generation = useRef(0)
  const busy = loading || submitting

  useEffect(() => {
    if (!open) {
      generation.current++
      setField('domain_suffix')
      setTarget('proxy')
      setValue('')
      setDrafts({})
      setNodeQuery('')
      setNodeTargetsOpen(false)
      setNodeTestStarted(false)
    }
  }, [open])

  if (!open) return null

  const matchGroup = MATCH_GROUPS.find((group) => (group.fields as readonly string[]).includes(field)) || MATCH_GROUPS[0]
  const fieldOption = RULE_FIELD_OPTIONS.find((option) => option.value === field)
  const error = valueError(field, value)
  const canSubmit = value.trim().length > 0 && !error
  const isNodeTarget = !TARGET_OPTIONS.some((option) => option.value === target)
  const preview = error ? null : rulePlainPreview(field, value, target)
  const query = nodeQuery.trim().toLowerCase()
  const filteredNodes = query ? nodeNames.filter((name) => name.toLowerCase().includes(query)) : nodeNames
  const hasTestingNode = nodeNames.some((name) => testingNodes[name])

  const handleFieldChange = (next: string) => {
    if (next === field) return
    setDrafts((current) => ({ ...current, [field]: value }))
    setField(next)
    setValue(drafts[next] ?? (next === 'protocol' ? 'quic' : ''))
  }

  const delayBadge = (name: string) => {
    if (testingNodes[name]) return <span className="node-delay testing">测速中…</span>
    const delay = delays[name]
    if (delay === undefined) return null
    return delay > 0
      ? <span className="node-delay ok num">{delay} ms</span>
      : <span className="node-delay timeout">超时</span>
  }

  const submit = async () => {
    if (!canSubmit || busy || inFlight.current) return
    inFlight.current = true
    setSubmitting(true)
    const submittedGeneration = generation.current
    try {
      const added = await onSubmit({ field, value: value.trim(), target })
      if (added && submittedGeneration === generation.current) onClose()
    } finally {
      inFlight.current = false
      setSubmitting(false)
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div ref={dialogRef} className="modal-card rule-modal" role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1} onClick={(event) => event.stopPropagation()}>
        <header className="rule-modal-header">
          <div className="rule-modal-heading">
            <ListFilter size={ICON.lg} className="icon-accent" aria-hidden="true" />
            <div><h3 id={titleId}>添加自定义规则</h3><p>指定网站或应用，选择它的连接方式</p></div>
          </div>
          <button className="icon-button" onClick={onClose} title="关闭 (Esc)" aria-label="关闭规则对话框"><X size={ICON.md} /></button>
        </header>

        <fieldset className="rule-modal-body" disabled={busy}>
          {busy && <p className="rule-submit-status" role="status">添加中，关闭窗口不会取消请求。</p>}
          <section className="rule-form-section" aria-labelledby="rule-match-title">
            <h4 id="rule-match-title">你想为谁设置规则？</h4>
            <div className="rule-kind-grid" role="radiogroup" aria-label="匹配对象">
              {MATCH_GROUPS.map((group) => (
                <button key={group.id} type="button" role="radio" aria-checked={matchGroup.id === group.id} className={classNames('rule-kind-option', matchGroup.id === group.id && 'active')} onClick={() => handleFieldChange(group.fields[0])}>
                  <strong>{group.label}</strong><span>{group.description}</span>
                </button>
              ))}
            </div>

            <div className="rule-field-row">
              <label htmlFor="rule-match-type">匹配方式</label>
              <select id="rule-match-type" value={field} onChange={(event) => handleFieldChange(event.target.value)}>
                {matchGroup.fields.map((name) => <option key={name} value={name}>{MATCH_LABELS[name]}</option>)}
              </select>
            </div>

            <div className="rule-value-row">
              <label htmlFor={valueId}>{VALUE_LABELS[field]}</label>
              {field === 'protocol' ? (
                <select id={valueId} value={value} onChange={(event) => setValue(event.target.value)} aria-label="规则值" data-autofocus>
                  {PROTOCOL_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
                </select>
              ) : (
                <input id={valueId} value={value} onChange={(event) => setValue(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && submit()} placeholder={fieldOption?.placeholder} aria-label="规则值" aria-invalid={!!error} aria-describedby={errorId} data-autofocus />
              )}
              <p id={errorId} className={classNames('rule-input-help', error && 'error')} role={error ? 'alert' : undefined}>{error || VALUE_HELP[field]}</p>
              {(field === 'domain_suffix' || field === 'domain') && (
                <div className="rule-examples" aria-label="常见网站">
                  <span>试试：</span>
                  {COMMON_DOMAIN_SITES.slice(0, 4).map((site) => <button type="button" key={site} onClick={() => setValue(site)}>{site}</button>)}
                </div>
              )}
              {field === 'process_name' && (
                <div className="rule-examples" aria-label="常见应用">
                  <span>试试：</span>
                  {PROCESS_EXAMPLES.map((app) => <button type="button" key={app.label} onClick={() => setValue(app.name)}>{app.label}</button>)}
                </div>
              )}
            </div>
          </section>

          <section className="rule-form-section" aria-labelledby="rule-target-title">
            <h4 id="rule-target-title">匹配后怎么连接？</h4>
            <div className="rule-target-grid" role="radiogroup" aria-label="规则目标">
              {TARGET_OPTIONS.map((option) => {
                const Icon = option.icon
                return (
                  <button key={option.value} type="button" role="radio" aria-checked={target === option.value} className={classNames('rule-target-option', option.value, target === option.value && 'active')} onClick={() => setTarget(option.value)}>
                    <Icon size={ICON.md} aria-hidden="true" /><strong>{option.label}</strong><span>{option.description}</span>
                  </button>
                )
              })}
            </div>

            <div className={classNames('rule-node-block', nodeTargetsOpen && 'open')}>
              <button type="button" className="rule-node-summary" aria-expanded={nodeTargetsOpen} aria-controls={nodeTargetsId} onClick={() => {
                const nextOpen = !nodeTargetsOpen
                setNodeTargetsOpen(nextOpen)
                if (nextOpen && !nodeTestStarted && !hasTestingNode && nodeNames.length > 0) {
                  setNodeTestStarted(true)
                  onTestNodes?.()
                }
              }}>
                <span>{isNodeTarget ? `指定节点：${target}` : '或指定一个节点'}</span><ChevronDown size={ICON.xs} aria-hidden="true" />
              </button>
              {nodeTargetsOpen && <div id={nodeTargetsId} className="rule-node-content">
                {nodeNames.length === 0 ? <div className="rule-node-empty">暂无可用节点</div> : <>
                  <div className="rule-node-search"><Search size={ICON.xs} aria-hidden="true" /><input value={nodeQuery} onChange={(event) => setNodeQuery(event.target.value)} placeholder="搜索节点" aria-label="搜索节点" /></div>
                  <div className="rule-node-list" role="radiogroup" aria-label="指定节点">
                    {filteredNodes.map((name) => <button key={name} type="button" role="radio" aria-checked={target === name} className={classNames('rule-node-row', target === name && 'active')} onClick={() => setTarget(name)}><span className="rule-node-name" title={name}>{name}</span>{delayBadge(name)}</button>)}
                    {filteredNodes.length === 0 && <div className="rule-node-empty">没有找到这个节点</div>}
                  </div>
                </>}
                {isNodeTarget && <div className="rule-node-warning"><TriangleAlert size={ICON.xs} aria-hidden="true" /><span>节点被删除或改名后，此规则会暂停生效；节点恢复后自动生效。</span></div>}
              </div>}
            </div>
          </section>

          <div className="rule-effect" aria-live="polite"><span>规则效果</span><p>{preview || '填写上方内容后，这里会显示规则的效果。'}</p></div>
          <p className="rule-priority-note">自定义规则优先于默认分流，在全局模式下也会生效。</p>
        </fieldset>

        <footer className="rule-modal-footer">
          <Button tone="ghost" size="sm" onClick={onClose}>{busy ? '关闭窗口' : '取消'}</Button>
          <Button tone="primary" size="sm" icon={<Plus size={ICON.xs} />} loading={busy} disabled={!canSubmit || busy} onClick={submit}>添加规则</Button>
        </footer>
      </div>
    </div>
  )
}
