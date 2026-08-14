import { memo } from 'react'
import { Server, Waypoints, Zap, LoaderCircle, Plus } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { 
  classNames, 
  formatDelay, 
  getDelayTone,
  protocolLabel 
} from '../utils.js'

const ProxyTile = memo(function ProxyTile({ nodeName, delay, isActive, isTesting, isSwitching, switchDisabled, onSwitchProxy, onTestDelay, group }) {
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
            ? <span className="proxy-node-name proxy-node-switching"><LoaderCircle size={12} className="spin" /><span>{nodeName}</span></span>
            : isActive
              ? <div className="proxy-tag"><span className="proxy-tag-dot" /><span>{nodeName}</span></div>
              : <span className="proxy-node-name">{nodeName}</span>}
        </div>
      </button>
      <button
        type="button"
        className={classNames('proxy-test-chip', getDelayTone(delay))}
        onClick={() => onTestDelay(nodeName)}
        disabled={isTesting}
        aria-label={`测试 ${nodeName} 延迟`}
      >
        {isTesting 
          ? <LoaderCircle size={10} className="spin" /> 
          : <Zap size={10} />}
        <span>{isTesting ? '测试中…' : formatDelay(delay)}</span>
      </button>
    </div>
  )
})

export function ProxyCard({ 
  status, 
  primaryGroup, 
  primaryGroupName, 
  currentNodeMeta,
  delays, 
  testingNodes, 
  testingGroup,
  switchingNode,
  onTestDelay, 
  onTestGroupDelays, 
  onSwitchProxy,
  onOpenAddNode
}) {
  const currentNodeDelay = primaryGroup?.now ? delays[primaryGroup.now] : undefined
  const isTestingCurrent = primaryGroup?.now ? testingNodes[primaryGroup.now] : false

  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <Waypoints size={14} className="section-icon" />
            <span>代理节点选择</span>
          </div>
          <Button 
            tone="secondary" 
            size="sm" 
            icon={<Zap size={12} />} 
            loading={testingGroup === primaryGroupName} 
            disabled={!primaryGroup || !status.running} 
            onClick={() => primaryGroup && onTestGroupDelays(primaryGroupName, primaryGroup.all)}
          >
            测试延迟
          </Button>
        </div>
      }
    >
      <button
        type="button"
        className="current-node-banner"
        onClick={() => primaryGroup?.now && onTestDelay(primaryGroup.now)}
        disabled={!primaryGroup?.now || isTestingCurrent}
        title={primaryGroup?.now ? '点击测试当前节点延迟' : undefined}
        aria-label={primaryGroup?.now ? `测试当前节点 ${primaryGroup.now} 延迟` : '当前节点'}
      >
        <div className="banner-icon-wrap"><Server size={18} className={classNames('banner-glyph', !primaryGroup?.now && 'idle')} /></div>
        <div className="banner-copy">
          <span className="banner-label">当前节点</span>
          <strong title={primaryGroup?.now || undefined}>{primaryGroup?.now || '未选择'}</strong>
          <span className="banner-meta">
            {currentNodeMeta
              ? `${currentNodeMeta.server}:${currentNodeMeta.server_port} · ${protocolLabel(currentNodeMeta.node_type)}`
              : primaryGroup 
                ? `来自代理组 ${primaryGroupName}` 
                : '等待服务启动'}
          </span>
        </div>
        <div className={classNames('banner-delay', getDelayTone(currentNodeDelay))}>
          {isTestingCurrent 
            ? <LoaderCircle size={20} className="spin" /> 
            : currentNodeDelay !== undefined && currentNodeDelay < 0
              ? <strong className="banner-delay-timeout">超时</strong>
              : <strong>{currentNodeDelay !== undefined ? currentNodeDelay : '--'}</strong>}
          {!isTestingCurrent && currentNodeDelay >= 0 && <span>ms</span>}
        </div>
      </button>

      <div className="proxy-grid-wrap">
        {primaryGroup ? (
          <div className="proxy-grid">
            {primaryGroup.all.map((nodeName) => (
              <ProxyTile
                key={nodeName}
                nodeName={nodeName}
                delay={delays[nodeName]}
                isActive={primaryGroup.now === nodeName}
                isTesting={Boolean(testingNodes[nodeName])}
                isSwitching={switchingNode === nodeName}
                switchDisabled={Boolean(switchingNode)}
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
              <Plus size={13} />
              <span>添加节点</span>
            </button>
          </div>
        ) : <div className="empty-block">服务未运行，暂时无法读取代理组。</div>}
      </div>
    </SectionCard>
  )
}
