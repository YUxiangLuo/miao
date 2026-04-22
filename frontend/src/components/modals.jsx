import { X, CircleAlert, Plus, LoaderCircle } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { 
  classNames, 
  CIPHER_OPTIONS, 
  VMESS_SECURITY_OPTIONS,
  VLESS_FLOW_OPTIONS,
  TUIC_CONGESTION_OPTIONS,
  EMPTY_NODE_FORM 
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

  const canSubmit = form.tag.trim() && form.server.trim() && form.server_port && form.password.trim()

  return (
    <div className="modal-overlay" onClick={onClose}>
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
          {['hysteria2', 'anytls', 'ss', 'trojan', 'vmess', 'vless', 'tuic'].map((value) => (
            <button
              key={value}
              className={classNames('tab-button', nodeType === value && 'active')}
              onClick={() => setNodeType(value)}
            >
              {value === 'ss' ? 'Shadowsocks' : value === 'anytls' ? 'AnyTLS' : value === 'vmess' ? 'VMess' : value === 'trojan' ? 'Trojan' : value === 'vless' ? 'VLESS' : value === 'tuic' ? 'TUIC' : 'Hysteria2'}
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
        ) : nodeType === 'vmess' ? (
          <>
            <div className="form-grid single">
              <label className="field">
                <span>加密方式</span>
                <select 
                  value={form.cipher} 
                  onChange={(event) => setForm((prev) => ({ ...prev, cipher: event.target.value }))}
                >
                  {VMESS_SECURITY_OPTIONS.map((sec) => (
                    <option key={sec} value={sec}>{sec}</option>
                  ))}
                </select>
              </label>
            </div>
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
        ) : nodeType === 'vless' ? (
          <>
            <div className="form-grid single">
              <label className="field">
                <span>Flow（可选）</span>
                <select 
                  value={form.cipher} 
                  onChange={(event) => setForm((prev) => ({ ...prev, cipher: event.target.value }))}
                >
                  {VLESS_FLOW_OPTIONS.map((flow) => (
                    <option key={flow} value={flow}>{flow || '无'}</option>
                  ))}
                </select>
              </label>
            </div>
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
        ) : nodeType === 'tuic' ? (
          <>
            <div className="form-grid single">
              <label className="field">
                <span>拥塞控制</span>
                <select 
                  value={form.cipher} 
                  onChange={(event) => setForm((prev) => ({ ...prev, cipher: event.target.value }))}
                >
                  {TUIC_CONGESTION_OPTIONS.map((cc) => (
                    <option key={cc} value={cc}>{cc}</option>
                  ))}
                </select>
              </label>
            </div>
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
        ) : (
          <>
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

        <div className="form-grid single">
          <label className="field">
            <span>{nodeType === 'vmess' || nodeType === 'vless' ? 'UUID' : nodeType === 'tuic' ? 'Token / UUID' : '密码'}</span>
            <input 
              value={form.password} 
              onChange={(event) => setForm((prev) => ({ ...prev, password: event.target.value }))} 
              placeholder={nodeType === 'vmess' || nodeType === 'vless' ? '例如：bf000d23-0752-40b4-affe-68f7707a9661' : nodeType === 'tuic' ? 'Token 或 UUID' : '密码'} 
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
          添加 {nodeType === 'ss' ? 'Shadowsocks' : nodeType === 'anytls' ? 'AnyTLS' : nodeType === 'vmess' ? 'VMess' : nodeType === 'trojan' ? 'Trojan' : nodeType === 'vless' ? 'VLESS' : nodeType === 'tuic' ? 'TUIC' : 'Hysteria2'} 节点
        </Button>
      </div>
    </div>
  )
}
