import { LoaderCircle } from 'lucide-react'
import { classNames } from '../utils.js'
import { LogoIcon } from './ui.jsx'

export function TopBar({ status, versionInfo, upgrading, onUpgradeClick, view = 'map', onViewChange }) {
  return (
    <header className="topbar">
      <div className="brand">
        <LogoIcon size={36} />
        <span className="brand-name">Miao</span>
      </div>
      {onViewChange && (
        <div className="view-switch" role="group" aria-label="视图">
          <button
            type="button"
            className={classNames('view-switch-option', view === 'map' && 'active')}
            aria-pressed={view === 'map'}
            onClick={() => onViewChange('map')}
          >
            地图
          </button>
          <button
            type="button"
            className={classNames('view-switch-option', view === 'panel' && 'active')}
            aria-pressed={view === 'panel'}
            onClick={() => onViewChange('panel')}
          >
            面板
          </button>
        </div>
      )}
      <div className="topbar-spacer" />
      <div className={classNames('run-badge', status.running ? 'running' : 'stopped')}>
        <span className="run-dot" />
        {status.running ? '运行中' : '已停止'}
      </div>
      <button 
        className={classNames('version-chip', versionInfo.has_update && 'has-update')} 
        onClick={onUpgradeClick} 
        disabled={upgrading || status.initializing}
      >
        {upgrading && <LoaderCircle size={12} className="spin" />}
        {!upgrading && versionInfo.has_update && <span className="version-dot" />}
        <span>{versionInfo.has_update ? versionInfo.latest : versionInfo.current || 'v--'}</span>
      </button>
    </header>
  )
}
