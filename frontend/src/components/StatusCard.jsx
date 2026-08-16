import { 
  ArrowUp, 
  ArrowDown,
  Activity,
  Globe2,
  LoaderCircle,
  Map,
  Power,
  PowerOff,
  Route
} from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { classNames, formatUptime, formatSpeed } from '../utils.js'

export function StatusCard({
  status,
  traffic,
  loadingAction,
  onToggleService,
  onSetRouteMode,
  onOpenConnections,
  onOpenMap
}) {
  const isGlobalMode = status.route_mode === 'global'
  const modeSwitching = loadingAction === 'routeMode'
  const modeControlDisabled = modeSwitching || status.initializing
  const serviceTone = status.initializing ? 'initializing' : status.running ? 'running' : 'stopped'
  const StatusIcon = status.initializing ? LoaderCircle : status.running ? Activity : PowerOff

  return (
    <SectionCard className="status-card" bodyClassName="status-card-body" header={null}>
      <div className="status-left-wrap">
        <div className={classNames('status-pill-icon', serviceTone)}>
          <StatusIcon size={18} className={classNames('status-pill-glyph', status.initializing && 'spin')} />
        </div>
        <div className="status-copy">
          <div className="status-title">
            Sing-box {status.initializing ? '初始化中' : status.running ? '运行中' : '已停止'}
          </div>
          <div className="status-subtitle">
            {status.running 
              ? `PID: ${status.pid ?? '--'} · 运行时长: ${formatUptime(status.uptime_secs)}` 
              : status.initializing 
                ? '正在获取订阅并启动服务…' 
                : '等待启动服务'}
          </div>
        </div>
      </div>

      <button
        type="button"
        className="traffic-chip"
        onClick={onOpenConnections}
        disabled={!status.running}
        title={status.running ? '查看链接统计' : '启动服务后可查看链接统计'}
      >
        <div className="traffic-item">
          <ArrowUp size={14} className="traffic-icon up" />
          <span>{formatSpeed(traffic.up)}</span>
        </div>
        <div className="traffic-item">
          <ArrowDown size={14} className="traffic-icon down" />
          <span>{formatSpeed(traffic.down)}</span>
        </div>
      </button>

      <div className="status-card-spacer" />
      <div className="route-mode-segment" role="group" aria-label="代理模式">
        <button
          type="button"
          className={classNames('route-mode-option', !isGlobalMode && 'active')}
          disabled={modeControlDisabled}
          aria-pressed={!isGlobalMode}
          onClick={() => {
            if (isGlobalMode) onSetRouteMode('rule')
          }}
        >
          <Route size={13} />
          <span>分流模式</span>
        </button>
        <button
          type="button"
          className={classNames('route-mode-option', isGlobalMode && 'active')}
          disabled={modeControlDisabled}
          aria-pressed={isGlobalMode}
          onClick={() => {
            if (!isGlobalMode) onSetRouteMode('global')
          }}
        >
          <Globe2 size={13} />
          <span>{modeSwitching ? '切换中' : '全局代理'}</span>
        </button>
      </div>
      <Button
        tone="ghost"
        icon={<Map size={14} />}
        onClick={onOpenMap}
        title="在世界地图上查看本机、代理节点与全部连接的地理分布"
      >
        地图模式
      </Button>
      <Button 
        tone={status.running ? 'danger' : 'success'} 
        icon={<Power size={14} />} 
        loading={loadingAction === 'start' || loadingAction === 'stop' || status.initializing} 
        disabled={loadingAction === 'start' || loadingAction === 'stop' || status.initializing} 
        onClick={onToggleService}
      >
        {status.running ? '停止服务' : '启动服务'}
      </Button>
    </SectionCard>
  )
}
