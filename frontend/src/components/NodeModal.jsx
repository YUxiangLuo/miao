import { useEffect, useId, useMemo, useState } from 'react'
import { ListPlus, Plus, Rocket, X } from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { Button } from './ui.jsx'
import { buildNodeRequest } from '../nodeForm.js'
import { parseShareLinks } from '../shareLink.js'
import {
  CIPHER_OPTIONS,
  CLIENT_FINGERPRINT_OPTIONS,
  classNames,
  EMPTY_NODE_FORM,
  HYSTERIA2_OBFS_OPTIONS,
  NODE_TYPE_OPTIONS,
  nodeCapabilities,
  nodeTypeDefaults,
  PACKET_ENCODING_OPTIONS,
  TRANSPORT_OPTIONS,
  TUIC_CONGESTION_OPTIONS,
  TUIC_UDP_RELAY_OPTIONS,
  VMESS_CIPHER_OPTIONS,
} from '../utils.js'

function previewChips(parsed) {
  const patch = parsed.formPatch
  const chips = []
  if (patch.tls_enabled && parsed.nodeType !== 'ss') chips.push('TLS')
  if (patch.reality_public_key) chips.push('Reality')
  if (patch.sni) chips.push(`SNI: ${patch.sni}`)
  if (patch.transport_type && patch.transport_type !== 'tcp') {
    chips.push(`传输: ${patch.transport_type}`)
  }
  if (patch.obfs_type) chips.push(`混淆: ${patch.obfs_type}`)
  if (patch.skip_cert_verify) chips.push('跳过证书验证')
  return chips
}

function ShareLinkItem({ item }) {
  if (!item.ok) {
    return (
      <div className="share-link-item error">
        <span className="share-link-item-line" title={item.line}>{item.line}</span>
        <span className="share-link-item-error">{item.error}</span>
      </div>
    )
  }

  const { parsed } = item
  const typeLabel = NODE_TYPE_OPTIONS.find((option) => option.value === parsed.nodeType)?.label
    || parsed.nodeType
  const chips = previewChips(parsed)

  return (
    <div className="share-link-item">
      <div className="share-link-item-top">
        <span className="share-link-type">{typeLabel}</span>
        <strong className="share-link-tag" title={parsed.tag}>{parsed.tag}</strong>
        <span className="share-link-server">{parsed.formPatch.server}:{parsed.formPatch.server_port}</span>
      </div>
      {chips.length > 0 && (
        <div className="share-link-chips">
          {chips.map((chip) => <span key={chip} className="share-link-chip">{chip}</span>)}
        </div>
      )}
    </div>
  )
}

function LinkImportPane({ onImport, loading }) {
  const [text, setText] = useState('')
  const [importing, setImporting] = useState(false)

  // 粘贴即解析,并顺带用 buildNodeRequest 做完整校验(密码长度等),问题直接显示在预览上
  const items = useMemo(() => parseShareLinks(text).map((item) => {
    if (!item.ok) return item
    try {
      const { nodeType, formPatch, tag } = item.parsed
      const payload = buildNodeRequest(nodeType, {
        ...EMPTY_NODE_FORM,
        ...nodeTypeDefaults(nodeType),
        ...formPatch,
        tag,
      })
      return { ...item, payload }
    } catch (error) {
      return { ...item, ok: false, error: error.message }
    }
  }), [text])

  const validPayloads = useMemo(
    () => items.filter((item) => item.ok).map((item) => item.payload),
    [items],
  )
  const busy = loading || importing

  const handleImport = async () => {
    if (!validPayloads.length || busy) return
    setImporting(true)
    try {
      await onImport(validPayloads)
    } finally {
      setImporting(false)
    }
  }

  return (
    <div className="node-pane">
      <textarea
        className="share-link-input"
        data-autofocus
        rows={5}
        value={text}
        onChange={(event) => setText(event.target.value)}
        placeholder={'粘贴节点分享链接,每行一条\n支持 hysteria2 / hy2 / ss / vmess / vless / trojan / tuic / anytls'}
        aria-label="节点分享链接"
      />

      <div className="share-link-preview node-pane-scroll">
        {items.length > 0 ? (
          items.map((item, index) => (
            <ShareLinkItem key={`${item.line}-${index}`} item={item} />
          ))
        ) : (
          <div className="share-link-empty">粘贴后自动识别节点信息,支持一次粘贴多条</div>
        )}
      </div>

      <Button
        tone="primary"
        icon={<ListPlus size={14} />}
        loading={busy}
        disabled={!validPayloads.length || busy}
        onClick={handleImport}
      >
        {validPayloads.length > 0 ? `添加 ${validPayloads.length} 个节点` : '添加节点'}
      </Button>
    </div>
  )
}

function ManualPane({ nodeType, setNodeType, form, setForm, loading, onSubmit }) {
  const activeLabel = NODE_TYPE_OPTIONS.find((option) => option.value === nodeType)?.label || nodeType
  const caps = nodeCapabilities(nodeType)
  const requiresPassword = caps.password
  const requiresUuid = caps.uuid
  const supportsTransport = caps.transport
  const showsTlsToggle = caps.tlsToggle
  const showsTlsFields = nodeType !== 'ss' && (!showsTlsToggle || form.tls_enabled || form.reality_public_key.trim())
  const pathTransport = ['ws', 'http', 'h2'].includes(form.transport_type)
  const handleNodeTypeChange = (event) => {
    const value = event.target.value
    setNodeType(value)
    setForm((prev) => ({ ...prev, ...nodeTypeDefaults(value) }))
  }

  const canSubmit = form.tag.trim()
    && form.server.trim()
    && form.server_port
    && (!requiresPassword || form.password.trim())
    && (!requiresUuid || form.uuid.trim())
    && (nodeType !== 'hysteria2' || !form.obfs_type || form.obfs_password.trim())

  return (
    <div className="node-pane">
      <div className="node-pane-scroll">
        <div className="form-grid single">
          <label className="field">
            <span>协议</span>
            <select value={nodeType} onChange={handleNodeTypeChange}>
              {NODE_TYPE_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>{label}</option>
              ))}
            </select>
          </label>
        </div>

      <div className="form-grid single">
        <label className="field">
          <span>节点名称</span>
          <input
            value={form.tag}
            onChange={(event) => setForm((prev) => ({ ...prev, tag: event.target.value }))}
            placeholder="例如：我的节点"
          />
        </label>
      </div>

      <div className="form-grid two">
        <label className="field">
          <span>服务器地址</span>
          <input
            value={form.server}
            onChange={(event) => setForm((prev) => ({ ...prev, server: event.target.value }))}
            placeholder="example.com"
          />
        </label>
        <label className="field">
          <span>端口</span>
          <input
            type="number"
            min="1"
            max="65535"
            value={form.server_port}
            onChange={(event) => setForm((prev) => ({
              ...prev,
              server_port: event.target.value === '' ? '' : Number(event.target.value),
            }))}
            placeholder="443"
          />
        </label>
      </div>

      {requiresPassword && (
        <div className="form-grid single">
          <label className="field">
            <span>密码</span>
            <input
              type="password"
              autoComplete="new-password"
              value={form.password}
              onChange={(event) => setForm((prev) => ({ ...prev, password: event.target.value }))}
              placeholder="密码"
            />
          </label>
        </div>
      )}

      {requiresUuid && (
        <div className="form-grid single">
          <label className="field">
            <span>UUID</span>
            <input
              value={form.uuid}
              onChange={(event) => setForm((prev) => ({ ...prev, uuid: event.target.value }))}
              placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
            />
          </label>
        </div>
      )}

      {nodeType === 'ss' && (
        <div className="form-grid single">
          <label className="field">
            <span>加密方式</span>
            <select
              value={form.cipher}
              onChange={(event) => setForm((prev) => ({ ...prev, cipher: event.target.value }))}
            >
              {CIPHER_OPTIONS.map((cipher) => (
                <option key={cipher} value={cipher}>{cipher}</option>
              ))}
            </select>
          </label>
        </div>
      )}

      {showsTlsToggle && (
        <div className="form-grid single">
          <label className="field checkbox-field">
            <input
              type="checkbox"
              checked={form.tls_enabled}
              onChange={(event) => setForm((prev) => ({ ...prev, tls_enabled: event.target.checked }))}
            />
            <span>启用 TLS</span>
          </label>
        </div>
      )}

      <details className="advanced-details">
        <summary>高级选项</summary>
        <div className="advanced-details-body">
          {nodeType === 'vmess' && (
            <div className="form-grid two">
              <label className="field">
                <span>VMess security</span>
                <select
                  value={form.vmess_cipher}
                  onChange={(event) => setForm((prev) => ({ ...prev, vmess_cipher: event.target.value }))}
                >
                  {VMESS_CIPHER_OPTIONS.map((cipher) => (
                    <option key={cipher} value={cipher}>{cipher}</option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>Alter ID</span>
                <input
                  type="number"
                  value={form.alter_id}
                  onChange={(event) => setForm((prev) => ({ ...prev, alter_id: Number(event.target.value || 0) }))}
                  min="0"
                />
              </label>
            </div>
          )}

          {nodeType === 'vless' && (
            <div className="form-grid two">
              <label className="field">
                <span>Flow</span>
                <select
                  value={form.flow}
                  onChange={(event) => setForm((prev) => ({ ...prev, flow: event.target.value }))}
                >
                  <option value="">默认</option>
                  <option value="xtls-rprx-vision">xtls-rprx-vision</option>
                </select>
              </label>
              <label className="field">
                <span>Packet encoding</span>
                <select
                  value={form.packet_encoding}
                  onChange={(event) => setForm((prev) => ({ ...prev, packet_encoding: event.target.value }))}
                >
                  {PACKET_ENCODING_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
            </div>
          )}

          {nodeType === 'vmess' && (
            <div className="form-grid single">
              <label className="field">
                <span>Packet encoding</span>
                <select
                  value={form.packet_encoding}
                  onChange={(event) => setForm((prev) => ({ ...prev, packet_encoding: event.target.value }))}
                >
                  {PACKET_ENCODING_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
            </div>
          )}

          {showsTlsFields && (
            <>
              <div className="form-grid two">
                <label className="field">
                  <span>SNI（可选）</span>
                  <input
                    value={form.sni}
                    onChange={(event) => setForm((prev) => ({ ...prev, sni: event.target.value }))}
                    placeholder="留空使用服务器地址"
                  />
                </label>
                <label className="field">
                  <span>TLS 指纹</span>
                  <select
                    value={form.client_fingerprint}
                    onChange={(event) => setForm((prev) => ({ ...prev, client_fingerprint: event.target.value }))}
                  >
                    {CLIENT_FINGERPRINT_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>{option.label}</option>
                    ))}
                  </select>
                </label>
              </div>
              <div className="form-grid single">
                <label className="field checkbox-field">
                  <input
                    type="checkbox"
                    checked={form.skip_cert_verify}
                    onChange={(event) => setForm((prev) => ({ ...prev, skip_cert_verify: event.target.checked }))}
                  />
                  <span>跳过证书验证（不推荐）</span>
                </label>
              </div>
            </>
          )}

          {nodeType === 'vless' && (
            <div className="form-grid two">
              <label className="field">
                <span>Reality public key</span>
                <input
                  value={form.reality_public_key}
                  onChange={(event) => {
                    const publicKey = event.target.value
                    setForm((prev) => ({
                      ...prev,
                      reality_public_key: publicKey,
                      client_fingerprint: publicKey.trim() && !prev.client_fingerprint
                        ? 'chrome'
                        : prev.client_fingerprint,
                    }))
                  }}
                  placeholder="可选"
                />
              </label>
              <label className="field">
                <span>Reality short ID</span>
                <input
                  value={form.reality_short_id}
                  onChange={(event) => setForm((prev) => ({ ...prev, reality_short_id: event.target.value }))}
                  placeholder="可选"
                />
              </label>
            </div>
          )}

          {supportsTransport && (
            <>
              <div className="form-grid single">
                <label className="field">
                  <span>传输层</span>
                  <select
                    value={form.transport_type}
                    onChange={(event) => setForm((prev) => ({ ...prev, transport_type: event.target.value }))}
                  >
                    {TRANSPORT_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>{option.label}</option>
                    ))}
                  </select>
                </label>
              </div>
              {pathTransport && (
                <div className="form-grid two">
                  <label className="field">
                    <span>路径</span>
                    <input
                      value={form.transport_path}
                      onChange={(event) => setForm((prev) => ({ ...prev, transport_path: event.target.value }))}
                      placeholder="/ws"
                    />
                  </label>
                  <label className="field">
                    <span>Host</span>
                    <input
                      value={form.transport_host}
                      onChange={(event) => setForm((prev) => ({ ...prev, transport_host: event.target.value }))}
                      placeholder="可选"
                    />
                  </label>
                </div>
              )}
              {form.transport_type === 'grpc' && (
                <div className="form-grid single">
                  <label className="field">
                    <span>gRPC service name</span>
                    <input
                      value={form.grpc_service_name}
                      onChange={(event) => setForm((prev) => ({ ...prev, grpc_service_name: event.target.value }))}
                      placeholder="可选"
                    />
                  </label>
                </div>
              )}
            </>
          )}

          {nodeType === 'hysteria2' && (
            <div className="form-grid two">
              <label className="field">
                <span>混淆类型</span>
                <select
                  value={form.obfs_type}
                  onChange={(event) => {
                    const obfsType = event.target.value
                    setForm((prev) => ({
                      ...prev,
                      obfs_type: obfsType,
                      obfs_password: obfsType ? prev.obfs_password : '',
                    }))
                  }}
                >
                  {HYSTERIA2_OBFS_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>混淆密码</span>
                <input
                  type="password"
                  autoComplete="new-password"
                  value={form.obfs_password}
                  disabled={!form.obfs_type}
                  onChange={(event) => setForm((prev) => ({ ...prev, obfs_password: event.target.value }))}
                  placeholder={form.obfs_type ? 'obfs password' : '未启用'}
                />
              </label>
            </div>
          )}

          {nodeType === 'tuic' && (
            <div className="form-grid two">
              <label className="field">
                <span>拥塞控制</span>
                <select
                  value={form.tuic_congestion_control}
                  onChange={(event) => setForm((prev) => ({ ...prev, tuic_congestion_control: event.target.value }))}
                >
                  {TUIC_CONGESTION_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>UDP relay mode</span>
                <select
                  value={form.tuic_udp_relay_mode}
                  onChange={(event) => setForm((prev) => ({ ...prev, tuic_udp_relay_mode: event.target.value }))}
                >
                  {TUIC_UDP_RELAY_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
            </div>
          )}

          {nodeType === 'tuic' && (
            <div className="form-grid single">
              <label className="field checkbox-field">
                <input
                  type="checkbox"
                  checked={form.tuic_zero_rtt}
                  onChange={(event) => setForm((prev) => ({ ...prev, tuic_zero_rtt: event.target.checked }))}
                />
                <span>启用 0-RTT</span>
              </label>
            </div>
          )}
        </div>
      </details>
      </div>

      <Button
        tone="primary"
        loading={loading}
        icon={<Plus size={14} />}
        disabled={!canSubmit || loading}
        onClick={onSubmit}
      >
        添加 {activeLabel} 节点
      </Button>
    </div>
  )
}

function VpsDeployPane({ onDeploy, loading }) {
  const [ip, setIp] = useState('')
  const [password, setPassword] = useState('')
  const [deploying, setDeploying] = useState(false)
  const busy = loading || deploying
  const canDeploy = ip.trim().length > 0 && password.length > 0

  const handleDeploy = async () => {
    if (!canDeploy || busy) return
    setDeploying(true)
    try {
      await onDeploy({ ip: ip.trim(), password })
    } finally {
      setDeploying(false)
    }
  }

  return (
    <div className="node-pane">
      <div className="node-pane-scroll">
        <div className="form-grid single">
          <label className="field">
            <span>VPS IP 地址</span>
            <input
              value={ip}
              onChange={(event) => setIp(event.target.value)}
              placeholder="203.0.113.10"
              aria-label="VPS IP 地址"
            />
          </label>
        </div>
        <div className="form-grid single">
          <label className="field">
            <span>root 密码</span>
            <input
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder="root 登录密码"
              aria-label="root 密码"
            />
          </label>
        </div>
        <div className="vps-deploy-hint">
          密码仅用于本次部署,不会被保存。目标 VPS 需允许 root SSH 登录(端口 22),将自动安装并配置 Hysteria2 节点。
        </div>
      </div>
      <Button
        tone="primary"
        icon={<Rocket size={14} />}
        loading={busy}
        disabled={!canDeploy || busy}
        onClick={handleDeploy}
      >
        {busy ? '部署中,可能需要 1-2 分钟…' : '开始部署'}
      </Button>
    </div>
  )
}

export function NodeModal({ open, nodeType, setNodeType, form, setForm, loading, onClose, onSubmit, onImport, onDeployVps }) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [mode, setMode] = useState('link')

  // 关闭后重新打开时回到默认的链接导入模式
  useEffect(() => {
    if (!open) setMode('link')
  }, [open])

  if (!open) return null

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card node-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Plus size={18} className="icon-accent" />
            <h3 id={titleId}>添加节点</h3>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭节点对话框">
            <X size={16} />
          </button>
        </div>

        <div className="connections-pills node-modal-tabs" role="group" aria-label="添加方式">
          <button
            type="button"
            className={classNames('connections-pill', mode === 'link' && 'active')}
            aria-pressed={mode === 'link'}
            onClick={() => setMode('link')}
          >
            粘贴链接
          </button>
          <button
            type="button"
            className={classNames('connections-pill', mode === 'manual' && 'active')}
            aria-pressed={mode === 'manual'}
            onClick={() => setMode('manual')}
          >
            手动填写
          </button>
          <button
            type="button"
            className={classNames('connections-pill', mode === 'vps' && 'active')}
            aria-pressed={mode === 'vps'}
            onClick={() => setMode('vps')}
          >
            VPS 部署
          </button>
        </div>

        {mode === 'link' ? (
          <LinkImportPane onImport={onImport} loading={loading} />
        ) : mode === 'vps' ? (
          <VpsDeployPane onDeploy={onDeployVps} loading={loading} />
        ) : (
          <ManualPane
            nodeType={nodeType}
            setNodeType={setNodeType}
            form={form}
            setForm={setForm}
            loading={loading}
            onSubmit={onSubmit}
          />
        )}
      </div>
    </div>
  )
}
