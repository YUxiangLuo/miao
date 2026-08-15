import { memo, useState } from 'react'
import { ListFilter, Plus, Trash2 } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { RuleModal } from './RuleModal.jsx'
import { classNames } from '../utils.js'
import { describeRule, ruleTargetLabel } from '../ruleFormat.js'

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

export function RulesCard({ rules, isInitializing, loadingAction, onAddRule, onDeleteRule, adblockEnabled, onToggleAdblock }) {
  const [showRuleModal, setShowRuleModal] = useState(false)
  const adding = loadingAction === 'addRule'
  const adblockPending = loadingAction === 'toggleAdblock'

  return (
    <>
      <SectionCard
        bodyClassName="panel-body-tight"
        header={
          <div className="section-header">
            <div className="section-title-wrap">
              <ListFilter size={14} className="section-icon" />
              <span>自定义规则</span>
              <span className="counter-pill">{rules.length}</span>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={adblockEnabled}
              aria-label="去广告"
              title="拦截广告域名(连接层拦截,规则集内嵌)"
              className={classNames('toggle-switch', adblockEnabled && 'on')}
              disabled={isInitializing || adblockPending}
              aria-busy={adblockPending || undefined}
              onClick={() => onToggleAdblock(!adblockEnabled)}
            >
              <span className="toggle-switch-label">去广告</span>
              <span className="toggle-switch-track">
                <span className="toggle-switch-thumb" />
              </span>
            </button>
            <Button
              tone="secondary"
              size="sm"
              icon={<Plus size={12} />}
              disabled={isInitializing}
              onClick={() => setShowRuleModal(true)}
            >
              添加
            </Button>
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
        </div>
      </SectionCard>

      <RuleModal
        open={showRuleModal}
        loading={adding}
        onClose={() => setShowRuleModal(false)}
        onSubmit={onAddRule}
      />
    </>
  )
}
