import { useState } from 'react'
import { useDialog } from '../hooks/useDialog'
import { Button } from './ui'
import { classNames } from '../utils'
import type { ToastTone } from '../hooks/useApi'

export interface McpControlProps {
  enabled: boolean
  pending: boolean
  onToggle: (enabled: boolean) => void
  showToast: (message: string, tone?: ToastTone) => number
}

interface Agent {
  id: 'claude' | 'codex'
  label: string
  command: (url: string) => string
}

const AGENTS: Agent[] = [
  {
    id: 'claude',
    label: 'Claude',
    command: (url) => `claude mcp add --transport http miao ${url}`,
  },
  {
    id: 'codex',
    label: 'Codex',
    command: (url) => `codex mcp add miao --url ${url}`,
  },
]

function AgentIcon({ agent }: { agent: Agent['id'] }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={agent === 'claude'
        ? 'M17.3041 3.541h-3.6718l6.696 16.918H24Zm-10.6082 0L0 20.459h3.7442l1.3693-3.5527h7.0052l1.3693 3.5528h3.7442L10.5363 3.5409Zm-.3712 10.2232 2.2914-5.9456 2.2914 5.9456Z'
        : 'M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z'} />
    </svg>
  )
}

// 顶栏 MCP 控件：端点开关 + 各 agent 的接入命令
export function McpControl({ enabled, pending, onToggle, showToast }: McpControlProps) {
  const url = `${window.location.origin}/mcp`
  const [manualCopy, setManualCopy] = useState<{ text: string; label: string } | null>(null)
  const dialogRef = useDialog(Boolean(manualCopy), () => setManualCopy(null))
  const agentInstructions = `## Miao MCP

Miao 是基于 sing-box 的透明代理，MCP 提供状态查询、节点测速和配置管理。

- 接入：在面板启用 MCP，将 Streamable HTTP 端点 ${url} 添加为服务 miao；agent 必须能访问此地址。本文不会自动配置 MCP。
- 操作前先用 get_status 查看状态，用 tools/list 获取当前工具及参数；节点名从 list_nodes 获取，不要猜测。
- 排查网络时优先查询状态、连接和规则；test_delay 测的是节点延迟，不是下载速度；test_connectivity 从运行 Miao 的主机发起请求，不代表用户浏览器必然可用。
- 修改仅限用户要求的范围。对要求 confirm: true 的工具，须先获得用户明确确认；启停、删除、VPS 部署、关闭 MCP、升级等可能影响网络，不要自行尝试。
- 操作后读取状态核对结果；超时不等于失败，先确认实际结果再决定是否重试。不要启动第二个 Miao 实例来排错。
`

  const copyText = async (text: string, label: string) => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text)
      } else {
        // http 局域网访问不是安全上下文，clipboard API 不可用，退回 execCommand
        const textarea = document.createElement('textarea')
        const focused = document.activeElement
        textarea.value = text
        textarea.style.position = 'fixed'
        textarea.style.opacity = '0'
        document.body.appendChild(textarea)
        try {
          textarea.select()
          if (!document.execCommand('copy')) throw new Error('copy failed')
        } finally {
          textarea.remove()
          if (focused instanceof HTMLElement) focused.focus()
        }
      }
      showToast(`已复制 ${label}`, 'success')
    } catch {
      setManualCopy({ text, label })
    }
  }

  return (
    <>
    <div className="mcp-control" aria-label="MCP 控制">
      <span className="mcp-control-label" title="Model Context Protocol">MCP</span>
      <button
        type="button"
        role="switch"
        aria-checked={enabled}
        aria-label="MCP 端点开关"
        title={enabled ? `MCP 端点已开启：${url}` : '开启 MCP 端点（AI agent 可通过它操作面板）'}
        className={classNames('toggle-switch', enabled && 'on')}
        disabled={pending}
        aria-busy={pending || undefined}
        onClick={() => onToggle(!enabled)}
      >
        <span className="toggle-switch-track">
          <span className="toggle-switch-thumb" />
        </span>
      </button>
      <span className="mcp-control-divider" />
      <div className="mcp-agent-buttons">
        {AGENTS.map((agent) => {
          const command = agent.command(url)
          return (
            <button
              key={agent.id}
              type="button"
              className={classNames('mcp-agent-button', agent.id)}
              onClick={() => copyText(command, `${agent.label} MCP 添加命令`)}
              title={`复制 ${agent.label} MCP 添加命令：${command}`}
              aria-label={`复制 ${agent.label} MCP 添加命令`}
            >
              <AgentIcon agent={agent.id} />
            </button>
          )
        })}
      </div>
      <Button tone="ghost" size="sm" title="复制简短 Markdown，粘贴到 AGENTS.md" aria-label="复制 AGENTS.md 说明" onClick={() => copyText(agentInstructions, 'AGENTS.md 说明')}>AGENTS.md</Button>
    </div>
    {manualCopy && (
      <div className="modal-overlay" onClick={() => setManualCopy(null)}>
        <div ref={dialogRef} className="modal-card modal-confirm" role="dialog" aria-modal="true" aria-label="手动复制" tabIndex={-1} onClick={event => event.stopPropagation()}>
          <h3>{manualCopy.label}</h3>
          <p>请手动复制：命令在终端运行，Markdown 粘贴到 AGENTS.md。</p>
          <textarea aria-label="待复制文本" data-autofocus readOnly rows={6} value={manualCopy.text} onFocus={event => event.currentTarget.select()} />
          <div className="modal-actions"><Button tone="primary" onClick={() => setManualCopy(null)}>完成</Button></div>
        </div>
      </div>
    )}
    </>
  )
}
