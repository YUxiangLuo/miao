import { useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { ICON } from '../tokens'
import { classNames } from '../utils'
import type { ToastTone } from '../hooks/useApi'

export interface McpFloatProps {
  enabled: boolean
  pending: boolean
  onToggle: (enabled: boolean) => void
  showToast: (message: string, tone?: ToastTone) => number
}

// 首页右下角的 MCP 浮动控件：端点开关 + 一键复制地址
export function McpFloat({ enabled, pending, onToggle, showToast }: McpFloatProps) {
  const [copied, setCopied] = useState(false)
  const url = `${window.location.origin}/mcp`

  const copyUrl = async () => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(url)
      } else {
        // http 局域网访问不是安全上下文，clipboard API 不可用，退回 execCommand
        const textarea = document.createElement('textarea')
        textarea.value = url
        textarea.style.position = 'fixed'
        textarea.style.opacity = '0'
        document.body.appendChild(textarea)
        textarea.select()
        document.execCommand('copy')
        textarea.remove()
      }
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
      showToast(`已复制 MCP 地址：${url}`, 'success')
    } catch {
      showToast(`复制失败，请手动复制：${url}`, 'error')
    }
  }

  return (
    <div className="mcp-float">
      <span className="mcp-float-label" title="Model Context Protocol">MCP</span>
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
      <button
        type="button"
        className="icon-button subtle"
        onClick={copyUrl}
        title={`复制 MCP 地址：${url}`}
        aria-label="复制 MCP 地址"
      >
        {copied ? <Check size={ICON.xs} /> : <Copy size={ICON.xs} />}
      </button>
    </div>
  )
}
