import { useEffect, useId, useState } from 'react'
import { Ban, Globe, ListFilter, Plus, Search, TriangleAlert, X, Zap } from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { Button } from './ui.jsx'
import { PROTOCOL_OPTIONS, RULE_TARGET_OPTIONS, ruleFieldOptions } from '../ruleFormat.js'
import { COMMON_DOMAIN_SITES, COMMON_PROCESS_APPS, processNameFor } from '../ruleApps.js'
import { ruleJsonPreview, rulePlainPreview } from '../rulePreview.js'
import { classNames } from '../utils.js'

// 「匹配什么」的分组与一句话说明（标签/占位符仍取自 ruleFormat.js 的字段选项）
const FIELD_GROUPS = [
  { label: '网站 / 域名', fields: ['domain_suffix', 'domain', 'domain_keyword'] },
  { label: '应用 / 进程', fields: ['process_name', 'process_path'] },
  { label: '网络', fields: ['ip_cidr', 'source_ip_cidr', 'port', 'port_range', 'protocol'] },
]

const FIELD_DESCRIPTIONS = {
  domain_suffix: '整个站点及其子域名',
  domain: '只匹配这一个域名',
  domain_keyword: '域名里包含这个词就算',
  ip_cidr: '目标 IP 落在网段内',
  source_ip_cidr: '按设备来源 IP 匹配',
  port: '单个目标端口',
  port_range: '一段目标端口',
  protocol: '按嗅探出的协议匹配',
  process_name: '按可执行文件名匹配',
  process_path: '按可执行文件完整路径匹配',
}

const TARGET_CARDS = [
  { value: 'proxy', label: '代理', desc: '走当前节点出口', icon: Globe },
  { value: 'direct', label: '直连', desc: '不走代理，直接访问', icon: Zap },
  { value: 'reject', label: '拦截', desc: '直接拒绝连接', icon: Ban },
]

const PROCESS_FIELDS = ['process_name', 'process_path']
const DOMAIN_FIELDS = ['domain_suffix', 'domain']

export function RuleModal({ open, loading, onClose, onSubmit, nodeNames = [], platform = 'linux', delays = {}, testingNodes = {} }) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [field, setField] = useState('domain_suffix')
  const [target, setTarget] = useState('proxy')
  const [value, setValue] = useState('')
  // 每个字段各自记住未提交的输入：切换类型不会把上一个类型的值带过来,
  // 切回时值还在（protocol 未编辑过则回落默认 quic）
  const [drafts, setDrafts] = useState({})
  const [nodeQuery, setNodeQuery] = useState('')

  // 关闭后重新打开时重置为默认表单
  useEffect(() => {
    if (!open) {
      setField('domain_suffix')
      setTarget('proxy')
      setValue('')
      setDrafts({})
      setNodeQuery('')
    }
  }, [open])

  if (!open) return null

  const fieldOptions = ruleFieldOptions(platform)
  const fieldOption = fieldOptions.find((option) => option.value === field)
  const isProtocol = field === 'protocol'
  const isProcessField = PROCESS_FIELDS.includes(field)
  const isDomainField = DOMAIN_FIELDS.includes(field)
  // 目标不在内置三项里即为指定节点出口
  const isNodeTarget = !RULE_TARGET_OPTIONS.some((option) => option.value === target)
  const canSubmit = value.trim().length > 0

  const plainPreview = rulePlainPreview(field, value, target)
  const jsonPreview = ruleJsonPreview(field, value, target)

  const query = nodeQuery.trim().toLowerCase()
  const filteredNodes = query
    ? nodeNames.filter((name) => name.toLowerCase().includes(query))
    : nodeNames

  // 类型切换：存下当前字段的草稿,恢复目标字段的草稿;
  // protocol 未编辑过时给默认值 quic
  const handleFieldChange = (next) => {
    if (next === field) return
    setDrafts((d) => ({ ...d, [field]: value }))
    setField(next)
    setValue(drafts[next] ?? (next === 'protocol' ? 'quic' : ''))
  }

  const delayBadge = (name) => {
    if (testingNodes[name]) return <span className="node-delay testing">测速中…</span>
    const delay = delays[name]
    if (delay === undefined) return null
    return delay > 0
      ? <span className="node-delay ok num">{delay} ms</span>
      : <span className="node-delay timeout">超时</span>
  }

  const submit = async () => {
    if (!canSubmit || loading) return
    const added = await onSubmit({ field, value: value.trim(), target })
    if (added) onClose()
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card connections-modal rule-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="connections-header">
          <div className="connections-header-title">
            <ListFilter size={18} className="icon-accent" />
            <h3 id={titleId}>添加规则</h3>
            <span className="rule-modal-subtitle">规则按顺序优先于内置分流（国内直连 / 国外代理），全局模式下仍生效</span>
          </div>
          <div className="connections-header-actions">
            <button className="icon-button" onClick={onClose} title="关闭 (Esc)" aria-label="关闭规则对话框">
              <X size={16} />
            </button>
          </div>
        </header>

        <div className="rule-workbench">
          <div className="rule-columns">
            <section className="rule-step" aria-label="匹配条件">
              <h4 className="rule-step-title"><span className="rule-step-no">1</span>匹配什么</h4>
              {FIELD_GROUPS.map((group) => (
                <div className="rule-field-group" key={group.label}>
                  <div className="rule-group-label">{group.label}</div>
                  <div className="rule-type-strip" role="radiogroup" aria-label={`规则字段 · ${group.label}`}>
                    {group.fields.map((name) => {
                      const option = fieldOptions.find((o) => o.value === name)
                      return (
                        <button
                          key={name}
                          type="button"
                          role="radio"
                          aria-checked={field === name}
                          className={classNames('rule-type-chip', field === name && 'active')}
                          onClick={() => handleFieldChange(name)}
                        >
                          {option?.label}
                        </button>
                      )
                    })}
                  </div>
                </div>
              ))}

              <div className="rule-editor">
                <div className="rule-editor-head">
                  <span className="rule-editor-title">{fieldOption?.label}</span>
                  <span className="rule-editor-desc">{FIELD_DESCRIPTIONS[field]}</span>
                </div>
                <div className="rule-value-block">
                  {isProtocol ? (
                    <select
                      value={value}
                      onChange={(event) => setValue(event.target.value)}
                      aria-label="规则值"
                      data-autofocus
                    >
                      {PROTOCOL_OPTIONS.map((option) => (
                        <option key={option.value} value={option.value}>{option.label}</option>
                      ))}
                    </select>
                  ) : (
                    <input
                      value={value}
                      onChange={(event) => setValue(event.target.value)}
                      onKeyDown={(event) => event.key === 'Enter' && submit()}
                      placeholder={fieldOption?.placeholder}
                      aria-label="规则值"
                      data-autofocus
                    />
                  )}
                </div>

                {isProcessField && (
                  <div className="rule-chips-block">
                    {COMMON_PROCESS_APPS.map((group) => (
                      <div key={group.category} className="rule-chip-group">
                        <div className="rule-group-label">{group.category}</div>
                        <div className="rule-chips">
                          {group.apps.map((app) => {
                            const name = processNameFor(app, platform)
                            return (
                              <button
                                key={app.label}
                                type="button"
                                className={classNames('rule-chip', value === name && 'active')}
                                onClick={() => setValue(name)}
                              >
                                {app.label}
                              </button>
                            )
                          })}
                        </div>
                      </div>
                    ))}
                  </div>
                )}

                {isDomainField && (
                  <div className="rule-chips-block">
                    <div className="rule-chip-group">
                      <div className="rule-group-label">常见站点</div>
                      <div className="rule-chips">
                        {COMMON_DOMAIN_SITES.map((site) => (
                          <button
                            key={site}
                            type="button"
                            className={classNames('rule-chip', value === site && 'active')}
                            onClick={() => setValue(site)}
                          >
                            {site}
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </section>

            <section className="rule-step" aria-label="出口目标">
              <h4 className="rule-step-title"><span className="rule-step-no">2</span>怎么走</h4>
              <div className="rule-target-grid" role="radiogroup" aria-label="规则目标">
                {TARGET_CARDS.map((card) => {
                  const Icon = card.icon
                  return (
                    <button
                      key={card.value}
                      type="button"
                      role="radio"
                      aria-checked={target === card.value}
                      className={classNames('rule-target-card', card.value, target === card.value && 'active')}
                      onClick={() => setTarget(card.value)}
                    >
                      <Icon size={18} />
                      <span className="rule-target-label">{card.label}</span>
                      <span className="rule-target-desc">{card.desc}</span>
                    </button>
                  )
                })}
              </div>

              <div className="rule-node-block">
                <div className="rule-group-label">或指定节点出口</div>
                {nodeNames.length === 0 ? (
                  <div className="rule-node-empty">暂无可用节点</div>
                ) : (
                  <>
                    <div className="rule-node-search">
                      <Search size={12} />
                      <input
                        value={nodeQuery}
                        onChange={(event) => setNodeQuery(event.target.value)}
                        placeholder="搜索节点"
                        aria-label="搜索节点"
                      />
                    </div>
                    <div className="rule-node-list" role="radiogroup" aria-label="指定节点">
                      {filteredNodes.map((name) => (
                        <button
                          key={name}
                          type="button"
                          role="radio"
                          aria-checked={target === name}
                          className={classNames('rule-node-row', target === name && 'active')}
                          onClick={() => setTarget(name)}
                        >
                          <span className="rule-node-name" title={name}>{name}</span>
                          {delayBadge(name)}
                        </button>
                      ))}
                      {filteredNodes.length === 0 && (
                        <div className="rule-node-empty">无匹配节点</div>
                      )}
                    </div>
                  </>
                )}
              </div>

              {isNodeTarget && (
                <div className="rule-node-warning">
                  <TriangleAlert size={12} />
                  <span>若该节点日后消失（改名或订阅变更），此规则将暂停生效并在列表中标记，节点恢复后自动生效</span>
                </div>
              )}
            </section>
          </div>

          <footer className="rule-footer">
            <div className="rule-preview">
              {plainPreview ? (
                <>
                  <div className="rule-preview-plain">{plainPreview}</div>
                  <code className="rule-preview-json">{jsonPreview}</code>
                </>
              ) : (
                <div className="rule-preview-empty">填写匹配值后，这里会预览这条规则的效果</div>
              )}
            </div>
            <div className="rule-actions">
              <Button tone="ghost" size="sm" onClick={onClose}>取消</Button>
              <Button
                tone="primary"
                size="sm"
                icon={<Plus size={12} />}
                loading={loading}
                disabled={!canSubmit}
                onClick={submit}
              >
                添加规则
              </Button>
            </div>
          </footer>
        </div>
      </div>
    </div>
  )
}
