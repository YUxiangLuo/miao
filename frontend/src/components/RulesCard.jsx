import { memo, useState } from 'react'
import { ListFilter, Plus, Trash2 } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { classNames } from '../utils.js'
import { describeRule, RULE_FIELD_OPTIONS, RULE_TARGET_OPTIONS, ruleTargetLabel } from '../ruleFormat.js'

const RuleRow = memo(function RuleRow({ rule, onDelete, disabled }) {
  const display = describeRule(rule)
  return (
    <div className="list-row">
      <div className="list-row-content">
        {display.structured ? (
          <>
            <div className="list-row-title">
              <span className="rule-field-chip">{display.fieldLabel}</span>
              <span className="rule-value" title={display.value}>{display.value}</span>
            </div>
            <div className="list-row-meta">{display.field}</div>
          </>
        ) : (
          <>
            <div className="list-row-title rule-value" title={display.raw}>{display.raw}</div>
            <div className="list-row-meta">自定义 JSON 规则</div>
          </>
        )}
      </div>
      {display.structured && display.target && (
        <span className={classNames('rule-target-badge', display.target)}>
          {ruleTargetLabel(display.target)}
        </span>
      )}
      <button
        className="icon-button subtle"
        onClick={() => onDelete(rule)}
        disabled={disabled}
        aria-label={`删除规则 ${display.structured ? display.value : display.raw}`}
      >
        <Trash2 size={13} />
      </button>
    </div>
  )
})

export function RulesCard({ rules, isInitializing, loadingAction, onAddRule, onDeleteRule }) {
  const [field, setField] = useState('domain_suffix')
  const [target, setTarget] = useState('proxy')
  const [value, setValue] = useState('')

  const fieldOption = RULE_FIELD_OPTIONS.find((option) => option.value === field)
  const canAdd = value.trim().length > 0
  const adding = loadingAction === 'addRule'

  const handleAdd = async () => {
    if (!canAdd || adding || isInitializing) return
    const added = await onAddRule({ field, value: value.trim(), target })
    if (added) setValue('')
  }

  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <ListFilter size={14} className="section-icon" />
            <span>自定义规则</span>
            <span className="counter-pill">{rules.length}</span>
          </div>
        </div>
      }
    >
      <div className="list-stack">
        {rules.length === 0 && <div className="empty-block">暂无自定义规则</div>}
        {rules.map((rule) => (
          <RuleRow
            key={rule.index}
            rule={rule}
            onDelete={onDeleteRule}
            disabled={isInitializing}
          />
        ))}

        <div className="rule-add-form">
          <div className="rule-add-row">
            <select
              value={field}
              onChange={(event) => setField(event.target.value)}
              aria-label="规则字段"
            >
              {RULE_FIELD_OPTIONS.map((option) => (
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
            </select>
          </div>
          <div className="rule-add-row">
            <input
              value={value}
              onChange={(event) => setValue(event.target.value)}
              onKeyDown={(event) => event.key === 'Enter' && handleAdd()}
              placeholder={fieldOption?.placeholder}
              aria-label="规则值"
            />
            <Button
              tone="secondary"
              size="sm"
              icon={<Plus size={12} />}
              loading={adding}
              disabled={!canAdd || isInitializing}
              onClick={handleAdd}
            >
              添加
            </Button>
          </div>
          <div className="rule-add-hint">规则按顺序优先于内置分流(国内直连 / 国外代理)</div>
        </div>
      </div>
    </SectionCard>
  )
}
