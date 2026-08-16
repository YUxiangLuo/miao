import { useEffect, useId, useState } from 'react'
import { Plus, X } from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { Button } from './ui.jsx'
import { PROTOCOL_OPTIONS, RULE_TARGET_OPTIONS, ruleFieldOptions } from '../ruleFormat.js'

export function RuleModal({ open, loading, onClose, onSubmit, nodeNames = [], platform = 'linux', delays = {}, testingNodes = {} }) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [field, setField] = useState('domain_suffix')
  const [target, setTarget] = useState('proxy')
  const [value, setValue] = useState('')

  // 关闭后重新打开时重置为默认表单
  useEffect(() => {
    if (!open) {
      setField('domain_suffix')
      setTarget('proxy')
      setValue('')
    }
  }, [open])

  if (!open) return null

  const fieldOptions = ruleFieldOptions(platform)
  const isProtocol = field === 'protocol'
  const fieldOption = fieldOptions.find((option) => option.value === field)
  // 目标不在内置三项里即为指定节点出口
  const isNodeTarget = !RULE_TARGET_OPTIONS.some((option) => option.value === target)
  const canSubmit = value.trim().length > 0

  // 打开弹窗时已触发批量测速；把结果追加在节点选项后面
  const delaySuffix = (name) => {
    if (testingNodes[name]) return ' · 测速中…'
    const delay = delays[name]
    if (delay === undefined) return ''
    return delay > 0 ? ` · ${delay} ms` : ' · 超时'
  }

  // 协议字段用下拉选值;切入时给默认值,切出时清空,避免把协议名带进文本字段
  const handleFieldChange = (event) => {
    const next = event.target.value
    setField(next)
    if (next === 'protocol') setValue('quic')
    else if (field === 'protocol') setValue('')
  }

  const handleSubmit = async () => {
    if (!canSubmit || loading) return
    const added = await onSubmit({ field, value: value.trim(), target })
    if (added) onClose()
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Plus size={18} className="icon-accent" />
            <h3 id={titleId}>添加规则</h3>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭规则对话框">
            <X size={16} />
          </button>
        </div>

        <div className="rule-modal-form">
          <div className="rule-add-row">
            <select
              value={field}
              onChange={handleFieldChange}
              aria-label="规则字段"
            >
              {fieldOptions.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
            <select
              value={target}
              onChange={(event) => setTarget(event.target.value)}
              aria-label="规则目标"
            >
              {RULE_TARGET_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
              {nodeNames.length > 0 && (
                <optgroup label="指定节点">
                  {nodeNames.map((name) => (
                    <option key={name} value={name}>{name}{delaySuffix(name)}</option>
                  ))}
                </optgroup>
              )}
            </select>
          </div>
          <div className="rule-add-row">
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
                onKeyDown={(event) => event.key === 'Enter' && handleSubmit()}
                placeholder={fieldOption?.placeholder}
                aria-label="规则值"
                data-autofocus
              />
            )}
          </div>
          <div className="rule-add-hint">规则按顺序优先于内置分流(国内直连 / 国外代理),全局模式下仍生效</div>
          {isNodeTarget && (
            <div className="rule-add-hint">若该节点日后消失(改名或订阅变更),此规则将暂停生效并在列表中标记,节点恢复后自动生效</div>
          )}
          <div className="modal-actions">
            <Button tone="ghost" size="sm" onClick={onClose}>取消</Button>
            <Button
              tone="secondary"
              size="sm"
              icon={<Plus size={12} />}
              loading={loading}
              disabled={!canSubmit}
              onClick={handleSubmit}
            >
              添加规则
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
