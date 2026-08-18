import { memo, useMemo, useState } from 'react'
import { ListFilter, Plus, Trash2, TriangleAlert } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { RuleModal } from './RuleModal.jsx'
import { classNames } from '../utils.js'
import { describeRule, ruleTargetLabel } from '../ruleFormat.js'
import { activeRuleIndexes } from '../ruleActivity.js'

const RuleRow = memo(function RuleRow({ rule, onDelete, disabled, active }) {
  const display = describeRule(rule)
  return (
    <div
      className={classNames('list-row', rule.skipped && 'skipped', active && 'rule-active')}
      title={active ? '该规则正在匹配连接' : undefined}
    >
      <div className="list-row-content">
        {display.structured ? (
          <>
            <div className="list-row-title structured">
              <span className={classNames('badge', 'rule-field-chip')}>{display.fieldLabel}</span>
              <span className="rule-value" title={display.value}>{display.value}</span>
            </div>
          </>
        ) : (
          <>
            <div className="list-row-title rule-value" title={display.raw}>{display.raw}</div>
            <div className="list-row-meta">自定义 JSON 规则</div>
          </>
        )}
      </div>
      {rule.skipped && (
        <span
          className="rule-skipped-icon"
          title="出口节点不存在,该规则未生效;请删除后重新添加"
          aria-label="规则未生效"
        >
          <TriangleAlert size={12} />
        </span>
      )}
      {display.structured && display.target && (
        <span className={classNames('badge', 'rule-target-badge', ['proxy', 'direct', 'reject'].includes(display.target) ? display.target : 'node')}>
          {ruleTargetLabel(display.target)}
        </span>
      )}
      <button
        className="icon-button subtle"
        onClick={() => onDelete(rule)}
        disabled={disabled}
        aria-label={`删除规则 ${display.structured ? display.value : display.raw}`}
      >
        <Trash2 size={12} />
      </button>
    </div>
  )
})

export function RulesCard({
  rules,
  isInitializing,
  loadingAction,
  onAddRule,
  onDeleteRule,
  nodeNames = [],
  connections = [],
  platform = 'linux',
  delays = {},
  testingNodes = {},
  onTestNodes,
}) {
  const [showRuleModal, setShowRuleModal] = useState(false)
  const adding = loadingAction === 'addRule'
  const activeIndexes = useMemo(
    () => activeRuleIndexes(rules, connections),
    [rules, connections],
  )

  return (
    <>
      <SectionCard
        bodyClassName="panel-body-tight"
        header={
          <div className="section-header">
            <div className="section-title-wrap">
              <ListFilter size={14} className="section-icon" />
              <span>自定义规则</span>
              <span className={classNames('badge', 'counter-pill')}>{rules.length}</span>
            </div>
            <Button
              tone="secondary"
              size="sm"
              icon={<Plus size={12} />}
              disabled={isInitializing}
              onClick={() => {
                setShowRuleModal(true)
                // 打开弹窗即测全部候选节点，延迟结果显示在下拉选项里
                onTestNodes?.()
              }}
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
              active={activeIndexes.has(rule.index)}
            />
          ))}
        </div>
      </SectionCard>

      <RuleModal
        open={showRuleModal}
        loading={adding}
        onClose={() => setShowRuleModal(false)}
        onSubmit={onAddRule}
        nodeNames={nodeNames}
        platform={platform}
        delays={delays}
        testingNodes={testingNodes}
      />
    </>
  )
}
