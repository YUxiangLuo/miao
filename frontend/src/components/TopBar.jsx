import { LoaderCircle } from 'lucide-react'
import { classNames } from '../utils.js'

// 极简 Logo - 圆环节点（紫色版）
function LogoIcon({ size = 20 }) {
  return (
    <svg 
      width={size} 
      height={size} 
      viewBox="0 0 64 64" 
      xmlns="http://www.w3.org/2000/svg"
      className="brand-icon"
    >
      <circle cx="32" cy="32" r="24" fill="none" stroke="#a78bfa" strokeWidth="3"/>
      <circle cx="32" cy="32" r="8" fill="#a78bfa"/>
    </svg>
  )
}

export function TopBar({ status, versionInfo, upgrading, onUpgradeClick }) {
  return (
    <header className="topbar">
      <div className="brand">
        <LogoIcon size={22} />
        <span className="brand-name">Miao</span>
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
