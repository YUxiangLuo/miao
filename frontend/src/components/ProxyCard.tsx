import { memo, useState } from 'react'
import { Waypoints, Zap, LoaderCircle, Plus } from 'lucide-react'
import { ICON } from '../tokens'
import { Button, SectionCard } from './ui'
import { 
  classNames, 
  formatDelay, 
  getDelayTone,
  protocolTone,
} from '../utils'
import type { NodeSelect, StatusData } from '../types/api'
import type { ClashProxy } from '../types/clash'

const NODE_SELECT_OPTIONS: Array<{ value: NodeSelect; label: string }> = [
  { value: 'manual', label: '手动选择' },
  { value: 'fastest_hk', label: '香港最快' },
  { value: 'fastest_jp', label: '日本最快' },
  { value: 'fastest_tw', label: '台湾最快' },
  { value: 'fastest_sg', label: '新加坡最快' },
  { value: 'fastest_us', label: '美国最快' },
]

export interface ProxyTileProps {
  nodeName: string
  protocol?: string
  delay?: number
  isActive: boolean
  isTesting: boolean
  isSwitching: boolean
  switchDisabled: boolean
  onSwitchProxy: (group: string, nodeName: string) => void
  onTestDelay: (nodeName: string) => void
  group: string
}

const ProxyTile = memo(function ProxyTile({ nodeName, protocol, delay, isActive, isTesting, isSwitching, switchDisabled, onSwitchProxy, onTestDelay, group }: ProxyTileProps) {
  return (
    <div className={classNames('proxy-tile', isActive && 'active')}>
      <button
        type="button"
        className="proxy-switch-button"
        aria-label={isActive ? `当前节点 ${nodeName}` : `切换到 ${nodeName}`}
        aria-pressed={isActive}
        disabled={isActive || isSwitching || switchDisabled}
        title={nodeName}
        onClick={() => onSwitchProxy(group, nodeName)}
      >
        <div className="proxy-tile-top">
          {isSwitching
            ? <span className="proxy-node-name proxy-node-switching"><LoaderCircle size={ICON.xs} className="spin" /><span>{nodeName}</span></span>
            : isActive
              ? <div className="proxy-tag"><span>{nodeName}</span></div>
              : <span className="proxy-node-name">{nodeName}</span>}
        </div>
        {protocol && <span className={classNames('badge', 'proxy-proto', protocolTone(protocol))}>{protocol}</span>}
      </button>
      <button
        type="button"
        className={classNames('proxy-test-btn', getDelayTone(delay))}
        onClick={() => onTestDelay(nodeName)}
        disabled={isTesting}
        aria-label={`测试 ${nodeName} 延迟`}
      >
        {isTesting
          ? <LoaderCircle size={ICON.lg} className="spin" />
          : <Zap size={ICON.lg} />}
        <span className="num">{isTesting ? '…' : formatDelay(delay)}</span>
      </button>
    </div>
  )
})

export interface ProxyCardProps {
  status: StatusData
  primaryGroup: ClashProxy | null
  primaryGroupName: string
  nodeProtocols?: Record<string, string>
  delays: Record<string, number>
  testingNodes: Record<string, boolean>
  testingGroup: string
  switchingNode: string
  nodeSelectPending: boolean
  onTestDelay: (nodeName: string) => void
  onTestGroupDelays: (groupName: string, nodeNames: string[]) => void
  onSwitchProxy: (group: string, nodeName: string) => void
  onSetNodeSelect?: (select: NodeSelect) => Promise<void> | void
  onOpenAddNode: () => void
}

export function ProxyCard({ 
  status, 
  primaryGroup, 
  primaryGroupName, 
  nodeProtocols = {},
  delays, 
  testingNodes, 
  testingGroup,
  switchingNode,
  nodeSelectPending,
  onTestDelay, 
  onTestGroupDelays, 
  onSwitchProxy,
  onSetNodeSelect,
  onOpenAddNode
}: ProxyCardProps) {
  const nodeSelect = status.node_select || 'manual'
  const isFastest = nodeSelect.startsWith('fastest_')
  // 受控 select 在 apply 期间停在用户选择上:status.node_select 要等热重启后的
  // fetchStatus 才更新,直接受控会弹回旧值;处理器结束(成功或失败)后回到服务端真值
  const [pendingSelect, setPendingSelect] = useState('')

  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <Waypoints size={ICON.sm} className="section-icon" />
            <span>节点列表</span>
          </div>
          <label className="node-select">
            <span className="node-select-label">节点选择</span>
            <select
              aria-label="节点选择"
              value={pendingSelect || nodeSelect}
              disabled={status.initializing || nodeSelectPending}
              onChange={(event) => {
                const next = event.target.value as NodeSelect
                setPendingSelect(next)
                Promise.resolve(onSetNodeSelect?.(next)).finally(() => setPendingSelect(''))
              }}
            >
              {NODE_SELECT_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </label>
          <Button 
            tone="secondary" 
            size="sm" 
            icon={<Zap size={ICON.xs} />} 
            loading={testingGroup === primaryGroupName} 
            disabled={!primaryGroup || !status.running} 
            onClick={() => primaryGroup && onTestGroupDelays(primaryGroupName, primaryGroup.all!)}
          >
            测试延迟
          </Button>
        </div>
      }
    >
      <div className="proxy-grid-wrap">
        {primaryGroup ? (
          <div className="proxy-grid">
            {/* primaryGroup 是 Selector/URLTest 分组，必有 all 候选列表 */}
            {primaryGroup.all!.map((nodeName: string) => (
              <ProxyTile
                key={nodeName}
                nodeName={nodeName}
                protocol={nodeProtocols[nodeName]}
                delay={delays[nodeName]}
                isActive={primaryGroup.now === nodeName}
                isTesting={Boolean(testingNodes[nodeName])}
                isSwitching={switchingNode === nodeName}
                switchDisabled={Boolean(switchingNode) || isFastest}
                group={primaryGroupName}
                onSwitchProxy={onSwitchProxy}
                onTestDelay={onTestDelay}
              />
            ))}
            <button
              className="proxy-tile add-tile"
              onClick={onOpenAddNode}
              disabled={status.initializing}
            >
              <Plus size={ICON.xs} />
              <span>添加节点</span>
            </button>
          </div>
        ) : <div className="empty-block">服务未运行，暂时无法读取代理组。</div>}
      </div>
    </SectionCard>
  )
}
