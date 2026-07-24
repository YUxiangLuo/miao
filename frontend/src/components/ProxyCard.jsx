import { memo } from 'react'
import { Radio, Zap, LoaderCircle, Plus } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { 
  classNames, 
  formatDelay, 
  getDelayTone,
  protocolLabel 
} from '../utils.js'

const ProxyTile = memo(function ProxyTile({ nodeName, delay, isActive, isTesting, onSwitchProxy, onTestDelay, group }) {
  return (
    <div className={classNames('proxy-tile', isActive && 'active')}>
      <button
        type="button"
        className="proxy-select-button"
        onClick={() => onSwitchProxy(group, nodeName)}
        disabled={isTesting}
        aria-pressed={isActive}
        aria-label={isActive ? `当前节点 ${nodeName}` : `切换到节点 ${nodeName}`}
      >
        <span className="proxy-tile-top">
          {isActive
            ? (
              <span className="proxy-tag">
                <span className="proxy-tag-dot" aria-hidden="true" />
                <span>{nodeName}</span>
              </span>
            )
            : <span className="proxy-node-name">{nodeName}</span>}
        </span>
      </button>
      <button 
        type="button"
        className={classNames('proxy-test-chip', getDelayTone(delay))} 
        onClick={() => onTestDelay(nodeName)}
        disabled={isTesting}
        aria-label={isTesting ? `正在测试节点 ${nodeName} 的延迟` : `测试节点 ${nodeName} 的延迟`}
      >
        {isTesting 
          ? <LoaderCircle size={10} className="spin" aria-hidden="true" />
          : <Zap size={10} aria-hidden="true" />}
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
  onTestDelay, 
  onTestGroupDelays, 
  onSwitchProxy,
  onOpenAddNode
}) {
  const currentNodeDelay = primaryGroup?.now ? delays[primaryGroup.now] : undefined
  const isTestingCurrent = primaryGroup?.now ? testingNodes[primaryGroup.now] : false

  return (
    <SectionCard
      className="proxy-card"
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <Radio size={14} className="section-icon" aria-hidden="true" />
            <h2 className="section-heading">代理节点选择</h2>
          </div>
          <Button 
            tone="secondary" 
            size="sm" 
            icon={<Zap size={12} aria-hidden="true" />}
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
        className="current-node-banner" 
        onClick={() => primaryGroup?.now && onTestDelay(primaryGroup.now)} 
        disabled={!primaryGroup?.now || Boolean(testingNodes[primaryGroup?.now])}
      >
        <div className="banner-icon-wrap"><span className="banner-dot" aria-hidden="true" /></div>
        <div className="banner-copy">
          <span className="banner-label">当前节点</span>
          <strong>{primaryGroup?.now || '未选择'}</strong>
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
            ? <LoaderCircle size={20} className="spin" aria-hidden="true" />
            : <strong>{currentNodeDelay !== undefined && currentNodeDelay >= 0 ? currentNodeDelay : '--'}</strong>}
          <span>ms</span>
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
                group={primaryGroupName}
                onSwitchProxy={onSwitchProxy}
                onTestDelay={onTestDelay}
              />
            ))}
            <button className="proxy-tile add-tile" onClick={onOpenAddNode}>
              <Plus size={13} aria-hidden="true" />
              <span>添加节点</span>
            </button>
          </div>
        ) : <div className="empty-block">服务未运行，暂时无法读取代理组。</div>}
      </div>
    </SectionCard>
  )
}
