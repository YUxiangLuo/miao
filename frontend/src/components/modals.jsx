import { X, CircleAlert, Plus, Activity, ArrowDown, ArrowUp, Network, RefreshCw, Route } from 'lucide-react'
import { Button } from './ui.jsx'
import { 
  classNames, 
  CIPHER_OPTIONS, 
  HYSTERIA2_OBFS_OPTIONS,
  formatBytes
} from '../utils.js'

export function ConfirmModal({ open, title, message, onCancel, onConfirm }) {
  if (!open) return null
  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal-card modal-confirm" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <CircleAlert size={18} className="icon-warning" />
            <h3>{title}</h3>
          </div>
          <button className="icon-button" onClick={onCancel}>
            <X size={16} />
          </button>
        </div>
        <p className="modal-message">{message}</p>
        <div className="modal-actions">
          <Button tone="ghost" size="sm" onClick={onCancel}>取消</Button>
          <Button tone="danger" size="sm" onClick={onConfirm}>确认</Button>
        </div>
      </div>
    </div>
  )
}

export function NodeModal({ open, nodeType, setNodeType, form, setForm, loading, onClose, onSubmit }) {
  if (!open) return null

  const canSubmit = form.tag.trim()
    && form.server.trim()
    && form.server_port
    && form.password.trim()
    && (nodeType !== 'hysteria2' || !form.obfs_type || form.obfs_password.trim())

  return (
    <div className="modal-overlay">
      <div className="modal-card" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Plus size={18} className="icon-accent" />
            <h3>添加节点</h3>
          </div>
          <button className="icon-button" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div className="tab-row">
          {['hysteria2', 'anytls', 'ss'].map((value) => (
            <button
              key={value}
              className={classNames('tab-button', nodeType === value && 'active')}
              onClick={() => setNodeType(value)}
            >
              {value === 'ss' ? 'Shadowsocks' : value === 'anytls' ? 'AnyTLS' : 'Hysteria2'}
            </button>
          ))}
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
              value={form.server_port}
              onChange={(event) => setForm((prev) => ({ ...prev, server_port: Number(event.target.value || 0) }))}
              placeholder="443"
            />
          </label>
        </div>

        {nodeType === 'ss' ? (
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
        ) : (
          <div className="form-grid single">
            <label className="field">
              <span>SNI（可选）</span>
              <input 
                value={form.sni} 
                onChange={(event) => setForm((prev) => ({ ...prev, sni: event.target.value }))} 
                placeholder="留空使用服务器地址" 
              />
            </label>
          </div>
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
                  value={form.obfs_password}
                  disabled={!form.obfs_type}
                  onChange={(event) => setForm((prev) => ({ ...prev, obfs_password: event.target.value }))}
                  placeholder={form.obfs_type ? 'obfs password' : '未启用'}
                />
              </label>
            </div>
          </>
        )}

        {nodeType !== 'ss' && (
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
        )}

        <div className="form-grid single">
          <label className="field">
            <span>密码</span>
            <input 
              value={form.password} 
              onChange={(event) => setForm((prev) => ({ ...prev, password: event.target.value }))} 
              placeholder="密码" 
            />
          </label>
        </div>

        <Button 
          tone="primary" 
          loading={loading} 
          icon={<Plus size={14} />} 
          disabled={!canSubmit || loading} 
          onClick={onSubmit}
        >
          添加 {nodeType === 'ss' ? 'Shadowsocks' : nodeType === 'anytls' ? 'AnyTLS' : 'Hysteria2'} 节点
        </Button>
      </div>
    </div>
  )
}

function countBy(items, mapper) {
  return items.reduce((acc, item) => {
    const key = mapper(item) || 'unknown'
    acc[key] = (acc[key] || 0) + 1
    return acc
  }, {})
}

function topEntries(counts, limit = 5) {
  return Object.entries(counts)
    .sort((a, b) => b[1] - a[1])
    .slice(0, limit)
}

function connectionTarget(connection) {
  const metadata = connection.metadata || {}
  const host = metadata.host || metadata.destinationIP || metadata.remoteDestination || metadata.destination
  const port = metadata.destinationPort || metadata.remoteDestinationPort
  if (!host) return 'unknown'
  return port ? `${host}:${port}` : host
}

function connectionOutbound(connection) {
  if (Array.isArray(connection.chains) && connection.chains.length > 0) {
    return connection.chains[connection.chains.length - 1]
  }
  return connection.rule || 'direct'
}

export function ConnectionsModal({ open, status, data, loading, error, onClose, onRefresh }) {
  if (!open) return null

  const connections = Array.isArray(data?.connections) ? data.connections : []
  const uploadTotal = Number(data?.uploadTotal || connections.reduce((sum, item) => sum + Number(item.upload || 0), 0))
  const downloadTotal = Number(data?.downloadTotal || connections.reduce((sum, item) => sum + Number(item.download || 0), 0))
  const networkCounts = topEntries(countBy(connections, (item) => item.metadata?.network), 4)
  const outboundCounts = topEntries(countBy(connections, connectionOutbound), 5)
  const topConnections = [...connections]
    .sort((a, b) => (Number(b.upload || 0) + Number(b.download || 0)) - (Number(a.upload || 0) + Number(a.download || 0)))
    .slice(0, 8)

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-card connections-modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Activity size={18} className="icon-accent" />
            <h3>连接统计</h3>
          </div>
          <div className="modal-title-actions">
            <button className="icon-button" onClick={onRefresh} disabled={loading || !status.running} title="刷新">
              <RefreshCw size={16} className={loading ? 'spin' : undefined} />
            </button>
            <button className="icon-button" onClick={onClose} title="关闭">
              <X size={16} />
            </button>
          </div>
        </div>

        {!status.running ? (
          <div className="connections-empty">服务未运行，暂无连接统计。</div>
        ) : (
          <>
            <div className="connection-stat-grid">
              <div className="connection-stat">
                <span>活跃连接</span>
                <strong>{connections.length}</strong>
              </div>
              <div className="connection-stat">
                <span>累计上传</span>
                <strong>{formatBytes(uploadTotal)}</strong>
              </div>
              <div className="connection-stat">
                <span>累计下载</span>
                <strong>{formatBytes(downloadTotal)}</strong>
              </div>
              <div className="connection-stat">
                <span>总流量</span>
                <strong>{formatBytes(uploadTotal + downloadTotal)}</strong>
              </div>
            </div>

            {error && <div className="connections-error">{error}</div>}

            <div className="connections-split">
              <div className="connections-panel">
                <div className="connections-panel-title">
                  <Network size={14} />
                  <span>协议分布</span>
                </div>
                {networkCounts.length > 0 ? networkCounts.map(([name, count]) => (
                  <div className="connection-count-row" key={name}>
                    <span>{name}</span>
                    <strong>{count}</strong>
                  </div>
                )) : <div className="connections-muted">暂无数据</div>}
              </div>

              <div className="connections-panel">
                <div className="connections-panel-title">
                  <Route size={14} />
                  <span>出口分布</span>
                </div>
                {outboundCounts.length > 0 ? outboundCounts.map(([name, count]) => (
                  <div className="connection-count-row" key={name}>
                    <span title={name}>{name}</span>
                    <strong>{count}</strong>
                  </div>
                )) : <div className="connections-muted">暂无数据</div>}
              </div>
            </div>

            <div className="connections-table">
              <div className="connections-table-header">
                <span>目标</span>
                <span>出口</span>
                <span>上传</span>
                <span>下载</span>
              </div>
              {topConnections.length > 0 ? topConnections.map((connection, index) => (
                <div className="connections-table-row" key={connection.id || `${connectionTarget(connection)}-${index}`}>
                  <span title={connectionTarget(connection)}>{connectionTarget(connection)}</span>
                  <span title={connectionOutbound(connection)}>{connectionOutbound(connection)}</span>
                  <span><ArrowUp size={12} />{formatBytes(Number(connection.upload || 0))}</span>
                  <span><ArrowDown size={12} />{formatBytes(Number(connection.download || 0))}</span>
                </div>
              )) : <div className="connections-empty inline">暂无活跃连接</div>}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
