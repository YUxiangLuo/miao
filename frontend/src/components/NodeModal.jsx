import { useId } from 'react'
import { Plus, X } from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { Button } from './ui.jsx'
import {
  CIPHER_OPTIONS,
  CLIENT_FINGERPRINT_OPTIONS,
  HYSTERIA2_OBFS_OPTIONS,
  NODE_TYPE_OPTIONS,
  PACKET_ENCODING_OPTIONS,
  TRANSPORT_OPTIONS,
  TUIC_CONGESTION_OPTIONS,
  TUIC_UDP_RELAY_OPTIONS,
  VMESS_CIPHER_OPTIONS,
  nodeCapabilities,
  nodeTypeDefaults,
} from '../utils.js'

export function NodeModal({ open, nodeType, setNodeType, form, setForm, loading, onClose, onSubmit }) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)

  if (!open) return null

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
              data-autofocus
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
          <>
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
          </>
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
    </div>
  )
}
