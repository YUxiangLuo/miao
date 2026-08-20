import {
  ArrowUp,
  ArrowDown,
  Globe2,
  LoaderCircle,
  Moon,
  Route,
  Sun,
} from 'lucide-react'
import { ICON, LOGO_SIZE } from '../tokens'
import { SectionCard, LogoIcon } from './ui'
import { useTheme, type Theme } from '../hooks/useTheme'
import { classNames, formatDelay, formatSpeed, getDelayTone } from '../utils'
import type { RouteMode, StatusData, VersionInfo } from '../types/api'
import type { ClashProxy, ClashTraffic } from '../types/clash'

const THEME_LABEL: Record<Theme, string> = { light: '浅色', dark: '深色' }
const THEME_ICON: Record<Theme, typeof Sun> = { light: Sun, dark: Moon }

export interface TopBarProps {
  status: StatusData
  traffic: Partial<ClashTraffic>
  versionInfo: VersionInfo
  upgrading: boolean
  onUpgradeClick: () => void
  loadingAction: string
  onSetRouteMode: (mode: RouteMode) => void
  onOpenConnections: () => void
  primaryGroup: ClashProxy | null
  delays: Record<string, number>
  testingNodes: Record<string, boolean>
  onTestDelay?: (nodeName: string) => void
}

export function TopBar({
  status,
  traffic,
  versionInfo,
  upgrading,
  onUpgradeClick,
  loadingAction,
  onSetRouteMode,
  onOpenConnections,
  primaryGroup,
  delays,
  testingNodes,
  onTestDelay,
}: TopBarProps) {
  const upgradeSupported = versionInfo.upgrade_supported !== false
  const label = versionInfo.has_update ? versionInfo.latest : versionInfo.current || 'v--'
  const isGlobalMode = status.route_mode === 'global'
  const modeSwitching = loadingAction === 'routeMode'
  const modeControlDisabled = modeSwitching || status.initializing
  const currentNode = primaryGroup?.now
  const currentNodeDelay = currentNode ? delays?.[currentNode] : undefined
  const isTestingCurrent = currentNode ? Boolean(testingNodes?.[currentNode]) : false
  const { theme, toggle } = useTheme()
  const ThemeIcon = THEME_ICON[theme] || Moon

  return (
    <SectionCard className="status-card topbar" bodyClassName="status-card-body" header={null}>
      <div className="brand">
        <LogoIcon size={LOGO_SIZE.topbar} />
      </div>
      <div className="topbar-divider" />

      <div className="status-cluster">
        <button
          type="button"
          className="traffic-chip"
          onClick={onOpenConnections}
          disabled={!status.ready}
          title={status.ready ? '查看链接统计' : '代理就绪后可查看链接统计'}
        >
          <div className="traffic-item">
            <ArrowUp size={ICON.sm} className="traffic-icon up" />
            <span className="num">{formatSpeed(traffic.up)}</span>
          </div>
          <div className="traffic-item">
            <ArrowDown size={ICON.sm} className="traffic-icon down" />
            <span className="num">{formatSpeed(traffic.down)}</span>
          </div>
        </button>
      </div>
      <div className="topbar-divider" />

      <button
        type="button"
        className="current-node-chip"
        onClick={() => currentNode && onTestDelay?.(currentNode)}
        disabled={!currentNode || isTestingCurrent}
        title={currentNode ? '点击测试当前节点延迟' : '当前节点'}
        aria-label={currentNode ? `测试当前节点 ${currentNode} 延迟` : '当前节点'}
      >
        <strong className="current-node-chip-name" title={currentNode || undefined}>
          {currentNode || '未选择'}
        </strong>
        <span className={classNames('current-node-chip-delay', 'num', getDelayTone(currentNodeDelay))}>
          {isTestingCurrent ? <LoaderCircle size={ICON.sm} className="spin" /> : formatDelay(currentNodeDelay)}
        </span>
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
          <Route size={ICON.sm} />
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
          <Globe2 size={ICON.sm} />
          <span>{modeSwitching ? '切换中' : '全局代理'}</span>
        </button>
      </div>

      <button
        type="button"
        className="theme-toggle"
        onClick={toggle}
        aria-label={`主题：${THEME_LABEL[theme]}，点击切换`}
        title={`主题：${THEME_LABEL[theme]}，点击切换`}
      >
        <ThemeIcon size={ICON.sm} />
      </button>

      {upgradeSupported ? (
        <button
          type="button"
          className={classNames('version-chip', versionInfo.has_update && 'has-update')}
          onClick={onUpgradeClick}
          disabled={upgrading || status.initializing}
        >
          {upgrading && <LoaderCircle size={ICON.xs} className="spin" />}
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
