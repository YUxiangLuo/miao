import { memo, useEffect, useMemo, useRef, useState } from 'react'
import type { CSSProperties } from 'react'
import { Waypoints, Zap, LoaderCircle, Plus } from 'lucide-react'
import { ARRIVE_MS, ICON, SCAN_STAGGER_CAP } from '../tokens'
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
  isArriving: boolean
  isTesting: boolean
  isSwitching: boolean
  switchDisabled: boolean
  index: number
  onSwitchProxy: (group: string, nodeName: string) => void
  onTestDelay: (nodeName: string) => void
  group: string
}

const ProxyTile = memo(function ProxyTile({ nodeName, protocol, delay, isActive, isArriving, isTesting, isSwitching, switchDisabled, index, onSwitchProxy, onTestDelay, group }: ProxyTileProps) {
  return (
    <div
      className={classNames('proxy-tile', isActive && 'active', isArriving && 'arrive')}
      style={{ '--i': Math.min(index, SCAN_STAGGER_CAP) } as CSSProperties}
    >
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
  maxMultiplierPending?: boolean
  nodeSelectPending: boolean
  onTestDelay: (nodeName: string) => void
  onTestGroupDelays: (groupName: string, nodeNames: string[]) => void
  onSwitchProxy: (group: string, nodeName: string) => void
  onSetMaxMultiplier?: (maxMultiplier: string | null) => Promise<void> | void
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
  maxMultiplierPending = false,
  nodeSelectPending,
  onTestDelay, 
  onTestGroupDelays, 
  onSwitchProxy,
  onSetMaxMultiplier,
  onSetNodeSelect,
  onOpenAddNode
}: ProxyCardProps) {
  const nodeSelect = status.node_select || 'manual'
  // 地区筛空时 effective node_select 会回退 manual，但用户仍需能调高/取消倍率
  // 来恢复自动模式，因此启用状态必须看 requested strategy。
  const requestedNodeSelect = status.requested_node_select || nodeSelect
  const isFastest = requestedNodeSelect.startsWith('fastest_')
  // 受控 select 在 apply 期间停在用户选择上:status.node_select 要等配置激活后的
  // fetchStatus 才更新,直接受控会弹回旧值;处理器结束(成功或失败)后回到服务端真值
  const [pendingSelect, setPendingSelect] = useState('')
  // undefined = 无请求；null = 请求中的“不限”；string = 请求中的具体倍率。
  const [pendingMultiplier, setPendingMultiplier] = useState<string | null | undefined>(undefined)
  const multiplierOptions = useMemo(() => {
    const values = new Set(status.multiplier_options || [])
    if (status.max_multiplier) values.add(status.max_multiplier)
    return [...values].sort((left, right) => Number(left) - Number(right))
  }, [status.multiplier_options, status.max_multiplier])
  const displayedMultiplier = pendingMultiplier === undefined
    ? (status.max_multiplier ?? '')
    : (pendingMultiplier ?? '')

  // 切换到位检测：primaryGroup.now 变化（手动切换成功后的 refetch、自动模式
  // 轮询发现 URLTest 换节点，走同一条路径）时给新选中 tile 加 .arrive 脉冲，
  // ARRIVE_MS 后移除并交棒给 tileGlow 呼吸。
  // 守卫：首次填充（prev 为空）与轮询闪断恢复（旧节点须仍在候选列表内）不触发。
  const [arrivedNode, setArrivedNode] = useState('')
  const switchRef = useRef<{ now: string; timer: number }>({ now: '', timer: 0 })
  const groupNow = primaryGroup?.now ?? ''
  const groupAll = primaryGroup?.all

  useEffect(() => {
    const prev = switchRef.current
    const all = groupAll ?? []
    const isSwitch = Boolean(
      prev.now && groupNow && prev.now !== groupNow &&
      all.includes(prev.now) && all.includes(groupNow),
    )
    prev.now = groupNow
    if (!isSwitch) return
    window.clearTimeout(prev.timer)
    setArrivedNode(groupNow)
    prev.timer = window.setTimeout(() => setArrivedNode(''), ARRIVE_MS)
  }, [groupNow, groupAll])

  // 卸载时清掉未决的脉冲清除定时器
  useEffect(() => () => window.clearTimeout(switchRef.current.timer), [])

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
            <span className="node-select-label">最高倍率</span>
            <select
              aria-label="最高倍率"
              value={displayedMultiplier}
              disabled={!isFastest || status.initializing || maxMultiplierPending}
              title={isFastest ? '限制自动测速候选的最高倍率' : '仅在地区最快模式下生效'}
              onChange={(event) => {
                const next = event.target.value || null
                setPendingMultiplier(next)
                Promise.resolve(onSetMaxMultiplier?.(next))
                  .finally(() => setPendingMultiplier(undefined))
              }}
            >
              <option value="">不限</option>
              {multiplierOptions.map((value) => (
                <option key={value} value={value}>{value}x</option>
              ))}
            </select>
          </label>
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
            disabled={!primaryGroup || !status.ready}
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
            {primaryGroup.all!.map((nodeName: string, index: number) => (
              <ProxyTile
                key={nodeName}
                nodeName={nodeName}
                protocol={nodeProtocols[nodeName]}
                delay={delays[nodeName]}
                isActive={primaryGroup.now === nodeName}
                isArriving={arrivedNode === nodeName}
                isTesting={Boolean(testingNodes[nodeName])}
                isSwitching={switchingNode === nodeName}
                switchDisabled={Boolean(switchingNode) || isFastest}
                index={index}
                group={primaryGroupName}
                onSwitchProxy={onSwitchProxy}
                onTestDelay={onTestDelay}
              />
            ))}
            <button
              className="proxy-tile add-tile"
              style={{ '--i': Math.min(primaryGroup.all!.length, SCAN_STAGGER_CAP) } as CSSProperties}
              onClick={onOpenAddNode}
              disabled={status.initializing}
            >
              <Plus size={ICON.xs} />
              <span>添加节点</span>
            </button>
          </div>
        ) : (
          <div className="empty-block">
            {status.running && !status.ready
              ? '代理正在启动，节点列表稍后出现'
              : '服务未运行，暂时无法读取代理组。'}
          </div>
        )}
      </div>
    </SectionCard>
  )
}
