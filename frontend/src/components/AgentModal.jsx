import { useEffect, useId, useRef, useState } from 'react'
import {
  Bot,
  HardDriveDownload,
  LoaderCircle,
  Send,
  Settings2,
  ShieldCheck,
  Square,
  X,
} from 'lucide-react'
import { useAgent } from '../hooks/useAgent.js'
import { useDialog } from '../hooks/useDialog.js'
import { Button } from './ui.jsx'

function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return '--'
  return `${Math.ceil(bytes / 1024 / 1024)} MiB`
}

export function AgentModal({ open, onClose }) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const messagesEndRef = useRef(null)
  const setupFocusRef = useRef(null)
  const composerRef = useRef(null)
  const [provider, setProvider] = useState('openai')
  const [model, setModel] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [draft, setDraft] = useState('')
  const agent = useAgent(open)

  useEffect(() => {
    if (!open) {
      setApiKey('')
      setDraft('')
      return
    }
    const configuredProvider = agent.status?.provider
    if (configuredProvider) setProvider(configuredProvider)
    setModel(agent.status?.model || '')
  }, [open, agent.status])

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
  }, [agent.messages])

  useEffect(() => {
    if (!open) return
    if (agent.showSetup) setupFocusRef.current?.focus()
    else if (agent.phase.name === 'ready') composerRef.current?.focus()
  }, [open, agent.showSetup, agent.phase.name])

  if (!open) return null

  const submitConfig = async (event) => {
    event.preventDefault()
    if (!apiKey.trim()) return
    const saved = await agent.configure({ provider, model, apiKey })
    if (saved) setApiKey('')
  }

  const submitMessage = () => {
    if (agent.sendPrompt(draft)) setDraft('')
  }

  const handleComposerKeyDown = (event) => {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      submitMessage()
    }
  }

  const statusUnsupported = agent.status && !agent.status.supported
  const preparing = !agent.showSetup && !['ready', 'working', 'error', 'closed'].includes(agent.phase.name)

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card agent-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="agent-header">
          <div className="modal-title-wrap">
            <span className="agent-icon"><Bot size={19} /></span>
            <div>
              <h3 id={titleId}>Miao 智能助手</h3>
              <p>由 Pi Agent 提供一次性对话</p>
            </div>
          </div>
          <div className="agent-header-actions">
            {agent.status?.configured && !agent.showSetup && (
              <button className="icon-button" onClick={agent.reconfigure} aria-label="重新配置 AI Provider">
                <Settings2 size={15} />
              </button>
            )}
            <button className="icon-button" onClick={onClose} aria-label="关闭智能助手">
              <X size={16} />
            </button>
          </div>
        </header>

        <div className="agent-body">
          {agent.loading && !agent.status ? (
            <div className="agent-centered" role="status">
              <LoaderCircle size={24} className="spin" />
              <span>正在检查助手环境…</span>
            </div>
          ) : agent.error && !agent.status ? (
            <div className="agent-centered agent-unavailable" role="alert">
              <HardDriveDownload size={28} />
              <strong>无法启动智能助手</strong>
              <span>{agent.error}</span>
            </div>
          ) : statusUnsupported ? (
            <div className="agent-centered agent-unavailable">
              <HardDriveDownload size={28} />
              <strong>当前环境暂不可用</strong>
              <span>{agent.status.reason}</span>
              <small>
                /tmp 可用 {formatBytes(agent.status.available_space_bytes)} ·
                inode {Number.isFinite(agent.status.available_tmp_inodes) ? agent.status.available_tmp_inodes : '--'} ·
                可用内存 {formatBytes(agent.status.available_memory_bytes)}
              </small>
            </div>
          ) : agent.showSetup ? (
            <form className="agent-setup" onSubmit={submitConfig}>
              <div className="agent-setup-copy">
                <ShieldCheck size={22} />
                <div>
                  <strong>配置 AI Provider</strong>
                  <p>密钥只保存在本机的 0600 权限文件中，不会显示在页面或发送到聊天记录。</p>
                </div>
              </div>

              <label className="field">
                <span>Provider</span>
                <select ref={setupFocusRef} value={provider} onChange={(event) => setProvider(event.target.value)}>
                  {(agent.status?.providers || []).map((item) => (
                    <option key={item.id} value={item.id}>{item.label}</option>
                  ))}
                </select>
              </label>

              <label className="field">
                <span>模型 ID（可选）</span>
                <input
                  value={model}
                  onChange={(event) => setModel(event.target.value)}
                  placeholder="留空时使用 Provider 默认模型"
                  maxLength={256}
                />
              </label>

              <label className="field">
                <span>API Key</span>
                <input
                  type="password"
                  value={apiKey}
                  onChange={(event) => setApiKey(event.target.value)}
                  placeholder="输入 API Key"
                  autoComplete="new-password"
                  autoCapitalize="none"
                  spellCheck={false}
                  maxLength={4096}
                  required
                />
              </label>

              {agent.error && <div className="agent-alert error" role="alert">{agent.error}</div>}
              <Button type="submit" tone="primary" loading={agent.loading} disabled={!apiKey.trim()}>
                保存并启动助手
              </Button>
              <small className="agent-local-note">安全限制：MVP 仅允许通过本机 localhost/loopback 访问。</small>
            </form>
          ) : preparing ? (
            <div className="agent-centered" role="status">
              <LoaderCircle size={26} className="spin" />
              <strong>{agent.phase.message || '正在准备 Pi Agent…'}</strong>
              {agent.phase.name === 'downloading' && (
                <span>首次下载约 43 MiB，完成后会缓存到 /tmp</span>
              )}
            </div>
          ) : (
            <>
              <div className="agent-session-meta">
                <span className={`agent-state-dot ${agent.phase.name}`} />
                <span>{agent.phase.message}</span>
                {agent.activeModel && (
                  <code>{agent.activeModel.provider}/{agent.activeModel.model}</code>
                )}
              </div>

              <div className="agent-messages" role="log" aria-label="助手对话" aria-live="polite" aria-busy={agent.streaming}>
                {agent.messages.length === 0 && !agent.error && (
                  <div className="agent-empty">
                    <Bot size={26} />
                    <strong>有什么可以帮你？</strong>
                    <span>可以询问 Miao 配置、sing-box 使用或常见故障。请不要发送任何密钥或订阅链接。</span>
                  </div>
                )}
                {agent.messages.map((message) => (
                  <div key={message.id} className={`agent-message ${message.role}`}>
                    <span className="agent-message-role">{message.role === 'user' ? '你' : 'Pi'}</span>
                    <div className="agent-message-text">
                      {message.text || (message.pending ? <LoaderCircle size={15} className="spin" /> : '')}
                    </div>
                  </div>
                ))}
                <div ref={messagesEndRef} />
              </div>

              {agent.notice && <div className="agent-alert info">{agent.notice}</div>}
              {agent.error && <div className="agent-alert error" role="alert">{agent.error}</div>}

              <div className="agent-composer">
                <textarea
                  ref={composerRef}
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  onKeyDown={handleComposerKeyDown}
                  placeholder={agent.phase.name === 'ready' ? '输入消息，Enter 发送…' : '助手尚未就绪'}
                  maxLength={8000}
                  rows={2}
                  disabled={agent.phase.name !== 'ready' || agent.streaming}
                  aria-label="发送给智能助手的消息"
                />
                {agent.streaming ? (
                  <button className="agent-send-button stop" onClick={agent.abort} aria-label="停止生成">
                    <Square size={15} fill="currentColor" />
                  </button>
                ) : (
                  <button
                    className="agent-send-button"
                    onClick={submitMessage}
                    disabled={!draft.trim() || agent.phase.name !== 'ready'}
                    aria-label="发送消息"
                  >
                    <Send size={16} />
                  </button>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
