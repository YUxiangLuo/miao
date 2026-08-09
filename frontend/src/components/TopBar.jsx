import { LoaderCircle, MessageCircle } from 'lucide-react'
import { classNames } from '../utils.js'
import { LogoIcon } from './ui.jsx'

export function TopBar({ status, versionInfo, upgrading, onUpgradeClick, onOpenAgent }) {
  return (
    <header className="topbar">
      <div className="brand">
        <LogoIcon size={36} />
        <span className="brand-name">Miao</span>
        <button
          type="button"
          className="agent-trigger"
          onClick={onOpenAgent}
          aria-label="打开 Miao 智能助手"
          title="智能助手"
        >
          <MessageCircle size={16} />
        </button>
      </div>
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
