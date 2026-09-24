import { useEffect, useId, useState } from 'react'
import { Bot, KeyRound, Plus, RotateCw, Settings2, Trash2, X } from 'lucide-react'
import { ICON } from '../tokens'
import { classNames } from '../utils'
import { useDialog } from '../hooks/useDialog'
import { Button } from './ui'
import type { AiStatus, AiProviderView as AiProvider, AiSettingsView as AiSettings } from '../types/api'

type ProviderDraft = AiProvider & { api_key: string | null }
type SettingsDraft = Omit<AiSettings, 'providers'> & { providers: ProviderDraft[] }

const PRESETS = {
  openrouter: ['OpenRouter', 'https://openrouter.ai/api/v1'],
  fireworks: ['Fireworks', 'https://api.fireworks.ai/inference/v1'],
} as const

async function jsonRequest<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, { ...init, cache: 'no-store' })
  const body = (await response.json().catch(() => ({}))) as { error?: string }
  if (!response.ok) throw new Error(body.error || `请求失败（${response.status}）`)
  return body as T
}

function auth(token: string): HeadersInit {
  return { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }
}

function toDraft(settings: AiSettings): SettingsDraft {
  return { ...settings, providers: settings.providers.map((provider) => ({ ...provider, api_key: null })) }
}

export function AiGateway({ nodeNames }: { nodeNames: string[] }) {
  const [status, setStatus] = useState<AiStatus | null>(null)
  const [statusError, setStatusError] = useState('')
  const [open, setOpen] = useState(false)

  const loadStatus = async () => {
    try {
      setStatus(await jsonRequest<AiStatus>('/api/ai/status'))
      setStatusError('')
    } catch (error) {
      setStatusError(error instanceof Error ? error.message : '状态加载失败')
    }
  }
  useEffect(() => {
    void loadStatus()
    const timer = setInterval(() => void loadStatus(), 10000)
    return () => clearInterval(timer)
  }, [])

  return (
    <>
      <div className="list-row ai-gateway-row">
        <Bot size={ICON.sm} className="icon-accent" aria-hidden="true" />
        <div className="list-row-content">
          <div className="list-row-title">AI 网关</div>
          <div className={classNames('list-row-meta', statusError && 'error')}>
            {statusError ||
              status?.error ||
              (status ? `http://127.0.0.1:${status.port}/v1 · 出口：${status.outbound}` : '正在读取状态…')}
          </div>
        </div>
        <span
          className={classNames('badge', status?.ready ? 'success' : status?.enabled ? 'warning' : 'neutral')}
        >
          {status?.ready ? '已就绪' : status?.enabled ? '未就绪' : '已停用'}
        </span>
        <button className="icon-button subtle" aria-label="配置 AI 网关" onClick={() => setOpen(true)}>
          <Settings2 size={ICON.xs} />
        </button>
      </div>
      {open && (
        <AiGatewayModal
          status={status}
          nodeNames={nodeNames}
          onClose={() => setOpen(false)}
          onSaved={loadStatus}
        />
      )}
    </>
  )
}

function AiGatewayModal({
  status,
  nodeNames,
  onClose,
  onSaved,
}: {
  status: AiStatus | null
  nodeNames: string[]
  onClose: () => void
  onSaved: () => Promise<void>
}) {
  const titleId = useId()
  const dialogRef = useDialog(true, onClose)
  const [token, setToken] = useState('')
  const [draft, setDraft] = useState<SettingsDraft | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [stale, setStale] = useState(false)
  const [newKey, setNewKey] = useState('')
  const load = async () => {
    if (!token.trim() || busy) return
    setBusy(true)
    setError('')
    setStale(false)
    try {
      const fresh = await jsonRequest<AiSettings>('/api/ai/settings', { headers: auth(token.trim()) })
      setDraft(toDraft(fresh))
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : '加载失败')
    } finally {
      setBusy(false)
    }
  }
  const patchProvider = (index: number, patch: Partial<ProviderDraft>) =>
    setDraft(
      (current) =>
        current && {
          ...current,
          providers: current.providers.map((item, i) => (i === index ? { ...item, ...patch } : item)),
        },
    )
  const invalidPort =
    draft &&
    (![draft.port, draft.proxy_port].every(
      (port) => Number.isInteger(port) && port >= 1024 && port <= 65535,
    ) ||
      draft.port === draft.proxy_port)
  const validation = invalidPort
    ? '端口需为不同的 1024–65535 之间的整数。'
    : draft?.enabled &&
        (!draft.has_access_key ||
          draft.providers.length === 0 ||
          draft.providers.some(
            (p) =>
              !p.name.trim() ||
              !p.base_url.trim() ||
              (!p.has_key && !p.api_key?.trim()) ||
              p.models.length === 0 ||
              p.models.some((model) => !model.id.trim() || !model.upstream.trim()),
          ))
      ? '启用前请先生成客户端密钥，并为每个提供商填写密钥和至少一个模型。'
      : ''
  const save = async () => {
    if (!draft || validation || busy) return
    setBusy(true)
    setError('')
    setStale(false)
    try {
      await jsonRequest('/api/ai/settings', {
        method: 'POST',
        headers: auth(token.trim()),
        body: JSON.stringify({
          ...draft,
          providers: draft.providers.map(({ has_key, ...provider }) => ({
            ...provider,
            api_key: provider.api_key?.trim() || (has_key ? null : ''),
          })),
        }),
      })
      await loadAfterSave()
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : '保存失败'
      setError(message)
      setStale(/revision|版本|stale|冲突|重新加载/i.test(message))
    } finally {
      setBusy(false)
    }
  }
  const loadAfterSave = async () => {
    const fresh = await jsonRequest<AiSettings>('/api/ai/settings', { headers: auth(token.trim()) })
    setDraft(toDraft(fresh))
    await onSaved()
  }
  const rotate = async () => {
    if (
      !draft ||
      busy ||
      !confirm(
        draft.has_access_key ? '生成新密钥后，旧客户端密钥将立即失效。继续吗？' : '生成客户端访问密钥？',
      )
    )
      return
    setBusy(true)
    setError('')
    try {
      const result = await jsonRequest<{ key: string; revision: number }>('/api/ai/access-key', {
        method: 'POST',
        headers: auth(token.trim()),
      })
      setNewKey(result.key)
      if (result.revision !== draft.revision + 1) {
        setError('密钥已生成，但配置被其他页面修改。请重新加载后保存。')
        setStale(true)
      } else {
        setDraft(current => current && { ...current, revision: result.revision, has_access_key: true })
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : '生成失败')
    } finally {
      setBusy(false)
    }
  }
  const addProvider = (preset: keyof typeof PRESETS) => {
    const [name, base_url] = PRESETS[preset]
    setDraft(
      (current) =>
        current && {
          ...current,
          providers: [
            ...current.providers,
            {
              id: `${preset}-${Date.now()}`,
              name,
              base_url,
              has_key: false,
              api_key: null,
              responses: false,
              models: [],
            },
          ],
        },
    )
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card ai-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="rule-modal-header">
          <div className="rule-modal-heading">
            <Bot size={ICON.lg} className="icon-accent" />
            <div>
              <h3 id={titleId}>AI 网关</h3>
              <p>在 Miao 主机上提供 OpenAI 兼容接口</p>
            </div>
          </div>
          <button className="icon-button" aria-label="关闭 AI 网关对话框" onClick={onClose}>
            <X size={ICON.md} />
          </button>
        </header>
        <div className="ai-modal-body">
          {!draft ? (
            <section className="ai-auth">
              <p>读取和修改配置需要管理员令牌。令牌仅保存在当前页面内存中。</p>
              <label>
                管理员令牌
                <input
                  type="password"
                  value={token}
                  autoComplete="off"
                  data-autofocus
                  onChange={(e) => setToken(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && void load()}
                />
              </label>
              <p className="ai-help">
                令牌文件：<code>{status?.admin_token_path || '读取状态后显示'}</code>
                <br />
                远程面板需在运行 Miao 的机器上读取此文件，再粘贴到这里。
              </p>
              <Button
                tone="primary"
                size="sm"
                loading={busy}
                disabled={!token.trim()}
                onClick={() => void load()}
              >
                读取配置
              </Button>
            </section>
          ) : (
            <fieldset disabled={busy} className="ai-settings">
              <div className="ai-grid">
                <label>
                  运行状态
                  <select
                    value={draft.enabled ? 'on' : 'off'}
                    onChange={(e) => setDraft({ ...draft, enabled: e.target.value === 'on' })}
                  >
                    <option value="off">停用</option>
                    <option value="on">启用</option>
                  </select>
                </label>
                <label>
                  网关端口
                  <input
                    type="number"
                    min="1024"
                    max="65535"
                    value={draft.port}
                    onChange={(e) => setDraft({ ...draft, port: Number(e.target.value) })}
                  />
                </label>
                <label>
                  代理端口
                  <input
                    type="number"
                    min="1024"
                    max="65535"
                    value={draft.proxy_port}
                    onChange={(e) => setDraft({ ...draft, proxy_port: Number(e.target.value) })}
                  />
                </label>
                <label>
                  出口
                  <select
                    value={draft.outbound}
                    onChange={(e) => setDraft({ ...draft, outbound: e.target.value })}
                  >
                    <option value="proxy">跟随全局代理</option>
                    <option value="direct">直连</option>
                    {!['proxy', 'direct', ...nodeNames].includes(draft.outbound) && (
                      <option value={draft.outbound}>{draft.outbound}（已不可用）</option>
                    )}
                    {nodeNames.map((name) => (
                      <option key={name} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <p className="ai-help">
                接口地址（位于 Miao 主机）：<code>http://127.0.0.1:{draft.port}/v1</code>
              </p>
              <div className="ai-key-row">
                <div>
                  <strong>客户端访问密钥</strong>
                  <p>
                    {draft.has_access_key
                      ? '已生成。出于安全考虑，已有密钥不会再次显示。'
                      : '尚未生成，启用网关前必须生成。'}
                  </p>
                </div>
                <Button
                  tone="secondary"
                  size="sm"
                  icon={draft.has_access_key ? <RotateCw size={ICON.xs} /> : <KeyRound size={ICON.xs} />}
                  onClick={() => void rotate()}
                >
                  {draft.has_access_key ? '轮换' : '生成'}
                </Button>
              </div>
              {newKey && (
                <div className="ai-new-key" role="status">
                  <strong>请立即复制新密钥，仅显示到本次关闭窗口：</strong>
                  <code>{newKey}</code>
                </div>
              )}
              <div className="ai-provider-heading">
                <strong>提供商</strong>
                <span>
                  <button type="button" onClick={() => addProvider('openrouter')}>
                    + OpenRouter
                  </button>
                  <button type="button" onClick={() => addProvider('fireworks')}>
                    + Fireworks
                  </button>
                </span>
              </div>
              {draft.providers.map((provider, index) => (
                <div className="ai-provider" key={provider.id}>
                  <div className="ai-provider-title">
                    <strong>{provider.name || '未命名提供商'}</strong>
                    <button
                      type="button"
                      className="icon-button subtle"
                      aria-label={`删除提供商 ${provider.name}`}
                      onClick={() =>
                        setDraft({ ...draft, providers: draft.providers.filter((_, i) => i !== index) })
                      }
                    >
                      <Trash2 size={ICON.xs} />
                    </button>
                  </div>
                  <div className="ai-grid">
                    <label>
                      名称
                      <input
                        value={provider.name}
                        onChange={(e) => patchProvider(index, { name: e.target.value })}
                      />
                    </label>
                    <label>
                      Base URL
                      <input
                        value={provider.base_url}
                        onChange={(e) => patchProvider(index, { base_url: e.target.value })}
                      />
                    </label>
                    <label>
                      API 密钥
                      <input
                        type="password"
                        autoComplete="new-password"
                        value={provider.api_key || ''}
                        placeholder={provider.has_key ? '已保存；留空则保持不变' : '必填'}
                        onChange={(e) => patchProvider(index, { api_key: e.target.value })}
                      />
                    </label>
                    <label className="ai-check">
                      <input
                        type="checkbox"
                        checked={provider.responses}
                        onChange={(e) => patchProvider(index, { responses: e.target.checked })}
                      />
                      启用 Responses API
                    </label>
                  </div>
                  <div className="ai-models">
                    <strong>模型别名</strong>
                    {provider.models.map((model, modelIndex) => (
                      <div className="ai-model-row" key={modelIndex}>
                        <input
                          aria-label="模型别名"
                          placeholder="例如 gpt-4o"
                          value={model.id}
                          onChange={(e) =>
                            patchProvider(index, {
                              models: provider.models.map((m, i) =>
                                i === modelIndex ? { ...m, id: e.target.value } : m,
                              ),
                            })
                          }
                        />
                        <input
                          aria-label="上游模型"
                          placeholder="上游模型 ID"
                          value={model.upstream}
                          onChange={(e) =>
                            patchProvider(index, {
                              models: provider.models.map((m, i) =>
                                i === modelIndex ? { ...m, upstream: e.target.value } : m,
                              ),
                            })
                          }
                        />
                        <button
                          type="button"
                          className="icon-button subtle"
                          aria-label="删除模型"
                          onClick={() =>
                            patchProvider(index, {
                              models: provider.models.filter((_, i) => i !== modelIndex),
                            })
                          }
                        >
                          <Trash2 size={ICON.xs} />
                        </button>
                      </div>
                    ))}
                    <button
                      type="button"
                      className="ai-text-button"
                      onClick={() =>
                        patchProvider(index, { models: [...provider.models, { id: '', upstream: '' }] })
                      }
                    >
                      <Plus size={ICON.xs} />
                      添加模型
                    </button>
                  </div>
                </div>
              ))}
              {draft.providers.length === 0 && (
                <div className="empty-block">暂无提供商；停用状态可直接保存。</div>
              )}
            </fieldset>
          )}
          {(error || validation) && (
            <p className="ai-error" role="alert">
              {error || validation}{' '}
              {stale && (
                <button type="button" onClick={() => void load()}>
                  放弃草稿并重新加载
                </button>
              )}
            </p>
          )}
        </div>
        {draft && (
          <footer className="rule-modal-footer">
            <Button tone="ghost" size="sm" onClick={onClose}>
              取消
            </Button>
            <Button
              tone="primary"
              size="sm"
              loading={busy}
              disabled={!!validation}
              onClick={() => void save()}
            >
              保存配置
            </Button>
          </footer>
        )}
      </div>
    </div>
  )
}
