import { ArrowLeftRight } from 'lucide-react'
import { ICON, LOGO_SIZE } from '../tokens'
import { LogoIcon } from './ui'

export function WorkspaceSwitch({ page }: { page: 'proxy' | 'ai' }) {
  const target = page === 'proxy' ? 'AI 网关' : '代理'
  return (
    <a
      className="brand workspace-switch"
      href={page === 'proxy' ? '#ai' : '#proxy'}
      aria-label={`切换到${target}`}
      title={`当前：${page === 'proxy' ? '代理' : 'AI 网关'} · 点击切换到${target}`}
    >
      <LogoIcon size={LOGO_SIZE.topbar} />
      <ArrowLeftRight size={ICON.xs} className="workspace-switch-hint" aria-hidden="true" />
    </a>
  )
}
