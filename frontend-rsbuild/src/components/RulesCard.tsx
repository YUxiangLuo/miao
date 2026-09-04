import { memo, useMemo, useState } from 'react'
import { ListFilter, Plus, Trash2, TriangleAlert } from 'lucide-react'
import { ICON } from '../tokens'
import { Button, SectionCard } from './ui'
import { RuleModal } from './RuleModal'
import { classNames } from '../utils'
import { describeRule, ruleFieldTone, ruleTargetLabel } from '../ruleFormat'
import { activeRuleIndexes } from '../ruleActivity'
import type { RuleInfo, RuleRequest } from '../types/api'
import type { ClashConnection } from '../types/clash'

interface RuleRowProps {
  rule: RuleInfo
  onDelete: (rule: RuleInfo) => void
  disabled: boolean
  active: boolean
}

const RuleRow = memo(function RuleRow({ rule, onDelete, disabled, active }: RuleRowProps) {
  const display = describeRule(rule)
  return (
    <div
      className={classNames('list-row', 'rule-row', display.structured && display.target && 'has-target', rule.skipped && 'skipped', active && 'rule-active')}
      title={active ? '该规则正在匹配连接' : undefined}
    >
      <div className="list-row-content">
        {display.structured ? (
          <>
            <div className="list-row-title structured">
              <span className={classNames('badge', 'rule-field-chip', ruleFieldTone(display.field))} title={display.fieldLabel}>
                <span className="rule-field-text">{display.fieldLabel}</span>
              </span>
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
      {display.structured && display.target && (
        <span
          className={classNames('badge', 'rule-target-badge', ['proxy', 'direct', 'reject'].includes(display.target) ? display.target : 'node')}
          title={ruleTargetLabel(display.target)}
        >
          <span className="rule-target-text">{ruleTargetLabel(display.target)}</span>
        </span>
      )}
      <span className="rule-status-slot">
        {rule.skipped ? (
          <span
            className="rule-skipped-icon"
            title="出口节点不存在,该规则未生效;请删除后重新添加"
            aria-label="规则未生效"
          >
            <TriangleAlert size={ICON.xs} />
          </span>
        ) : active ? (
          <span className="rule-live-dot" title="正在匹配" aria-hidden="true" />
        ) : null}
      </span>
      <button
        className="icon-button subtle"
        onClick={() => onDelete(rule)}
        disabled={disabled}
        aria-label={`删除规则 ${display.structured ? display.value : display.raw}`}
      >
        <Trash2 size={ICON.xs} />
      </button>
    </div>
  )
})

export interface RulesCardProps {
  rules: RuleInfo[]
  isInitializing: boolean
  pendingActions: ReadonlySet<string>
  onAddRule: (rule: RuleRequest) => Promise<boolean>
  onDeleteRule: (rule: RuleInfo) => void
  nodeNames?: string[]
  connections?: ClashConnection[]
  platform?: string
  delays?: Record<string, number>
  testingNodes?: Record<string, boolean>
  onTestNodes?: () => void
}

export function RulesCard({
  rules,
  isInitializing,
  pendingActions,
  onAddRule,
  onDeleteRule,
  nodeNames = [],
  connections = [],
  platform = 'linux',
  delays = {},
  testingNodes = {},
  onTestNodes,
}: RulesCardProps) {
  const [showRuleModal, setShowRuleModal] = useState(false)
  const adding = pendingActions.has('addRule')
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
              <ListFilter size={ICON.sm} className="section-icon" />
              <span>自定义规则</span>
              <span className={classNames('badge', 'counter-pill')}>{rules.length}</span>
            </div>
            <Button
              tone="secondary"
              size="sm"
              icon={<Plus size={ICON.xs} />}
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
        onTestNodes={onTestNodes}
      />
    </>
  )
}
