import { LoaderCircle } from 'lucide-react'
import { classNames } from '../utils.js'
import { LogoIcon } from './ui.jsx'

export function TopBar({ status, versionInfo, upgrading, onUpgradeClick }) {
  const upgradeSupported = versionInfo.upgrade_supported !== false
  const label = versionInfo.has_update ? versionInfo.latest : versionInfo.current || 'v--'

  return (
    <header className="topbar">
      <div className="brand">
        <LogoIcon size={44} />
        <span className="brand-name">Miao</span>
      </div>
      <div className="topbar-spacer" />
      <div className={classNames('run-badge', status.running ? 'running' : 'stopped')}>
        <span className="run-dot" />
        {status.running ? '运行中' : '已停止'}
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
        <div className="version-chip" title="请通过安装包或便携包更新">
          <span>{versionInfo.current || 'v--'}</span>
        </div>
      )}
    </header>
  )
}
