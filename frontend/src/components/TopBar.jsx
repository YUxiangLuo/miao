import {
  ArrowUp,
  ArrowDown,
  Activity,
  Globe2,
  LoaderCircle,
  PowerOff,
  Route,
} from 'lucide-react'
import { SectionCard, LogoIcon } from './ui.jsx'
import { classNames, formatUptime, formatSpeed } from '../utils.js'

export function TopBar({
  status,
  traffic,
  versionInfo,
  upgrading,
  onUpgradeClick,
  loadingAction,
  onSetRouteMode,
  onOpenConnections,
}) {
  const upgradeSupported = versionInfo.upgrade_supported !== false
  const label = versionInfo.has_update ? versionInfo.latest : versionInfo.current || 'v--'
  const isGlobalMode = status.route_mode === 'global'
  const modeSwitching = loadingAction === 'routeMode'
  const modeControlDisabled = modeSwitching || status.initializing
  const serviceTone = status.initializing ? 'initializing' : status.running ? 'running' : 'stopped'
  const StatusIcon = status.initializing ? LoaderCircle : status.running ? Activity : PowerOff

  return (
    <SectionCard className="status-card topbar" bodyClassName="status-card-body" header={null}>
      <div className="brand">
        <LogoIcon size={30} />
        <span className="brand-name">Miao</span>
      </div>
      <div className="topbar-divider" />

      <div className="status-cluster">
        <div className="status-left-wrap">
          <div className={classNames('status-pill-icon', serviceTone)}>
            <StatusIcon size={18} className={classNames('status-pill-glyph', status.initializing && 'spin')} />
          </div>
          <div className="status-copy">
            <div className="status-title">
              Sing-box {status.initializing ? '初始化中' : status.running ? '运行中' : '已停止'}
            </div>
            <div className="status-subtitle num">
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
            <span className="num">{formatSpeed(traffic.up)}</span>
          </div>
          <div className="traffic-item">
            <ArrowDown size={14} className="traffic-icon down" />
            <span className="num">{formatSpeed(traffic.down)}</span>
          </div>
        </button>
      </div>
      <div className="topbar-divider" />

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
          <Route size={14} />
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
          <Globe2 size={14} />
          <span>{modeSwitching ? '切换中' : '全局代理'}</span>
        </button>
      </div>

      {upgradeSupported ? (
        <button
          type="button"
          className={classNames('version-chip', versionInfo.has_update && 'has-update')}
          onClick={onUpgradeClick}
          disabled={upgrading || status.initializing}
        >
          {upgrading && <LoaderCircle size={12} className="spin" />}
          {!upgrading && versionInfo.has_update && <span className="version-dot" />}
          <span>{label}</span>
        </button>
      ) : (
        <div
          className={classNames('version-chip', versionInfo.has_update && 'has-update')}
          title={
            versionInfo.has_update
              ? `发现新版本 ${versionInfo.latest}，请下载安装包更新`
              : '请下载安装包更新'
          }
        >
          {versionInfo.has_update && <span className="version-dot" />}
          <span>{label}</span>
        </div>
      )}
    </SectionCard>
  )
}
