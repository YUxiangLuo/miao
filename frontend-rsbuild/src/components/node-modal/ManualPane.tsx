import { Plus } from 'lucide-react'
import type { ChangeEvent, Dispatch, InputHTMLAttributes, SelectHTMLAttributes, SetStateAction } from 'react'
import { ICON } from '../../tokens'
import { Button } from '../ui'
import {
  CIPHER_OPTIONS,
  CLIENT_FINGERPRINT_OPTIONS,
  HYSTERIA2_OBFS_OPTIONS,
  NODE_TYPE_OPTIONS,
  nodeCapabilities,
  nodeTypeDefaults,
  PACKET_ENCODING_OPTIONS,
  TRANSPORT_OPTIONS,
  TUIC_CONGESTION_OPTIONS,
  TUIC_UDP_RELAY_OPTIONS,
  VMESS_CIPHER_OPTIONS,
  type NodeForm,
  type SelectOption,
} from '../../utils'
import type { NodeType } from '../../types/api'

const VLESS_FLOW_OPTIONS: SelectOption[] = [
  { value: '', label: '默认' },
  { value: 'xtls-rprx-vision', label: 'xtls-rprx-vision' },
]

// 部分选项常量是纯字符串数组,统一成 {value,label} 供 SelectField 渲染
const asOptions = (items: string[]): SelectOption[] => items.map((item) => ({ value: item, label: item }))

interface TextFieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string
}

function TextField({ label, ...inputProps }: TextFieldProps) {
  return (
    <label className="field">
      <span>{label}</span>
      <input {...inputProps} />
    </label>
  )
}

interface SelectFieldProps extends SelectHTMLAttributes<HTMLSelectElement> {
  label: string
  options: SelectOption[]
}

function SelectField({ label, options, ...selectProps }: SelectFieldProps) {
  return (
    <label className="field">
      <span>{label}</span>
      <select {...selectProps}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
    </label>
  )
}

function CheckboxField({ label, ...inputProps }: TextFieldProps) {
  return (
    <label className="field checkbox-field">
      <input type="checkbox" {...inputProps} />
      <span>{label}</span>
    </label>
  )
}

// 按值类型拆分的表单字段键：文本绑定只能用于 string 字段，checkbox 只能用于 boolean 字段
type StringField = { [K in keyof NodeForm]: NodeForm[K] extends string ? K : never }[keyof NodeForm]
type BooleanField = { [K in keyof NodeForm]: NodeForm[K] extends boolean ? K : never }[keyof NodeForm]

export interface ManualPaneProps {
  nodeType: NodeType
  setNodeType: Dispatch<SetStateAction<NodeType>>
  form: NodeForm
  setForm: Dispatch<SetStateAction<NodeForm>>
  loading: boolean
  onSubmit: () => void
}

export function ManualPane({ nodeType, setNodeType, form, setForm, loading, onSubmit }: ManualPaneProps) {
  const activeLabel = NODE_TYPE_OPTIONS.find((option) => option.value === nodeType)?.label || nodeType
  const caps = nodeCapabilities(nodeType)
  const requiresPassword = caps.password
  const requiresUuid = caps.uuid
  const supportsTransport = caps.transport
  const showsTlsToggle = caps.tlsToggle
  const showsTlsFields = nodeType !== 'ss' && (!showsTlsToggle || form.tls_enabled || form.reality_public_key.trim())
  const pathTransport = ['ws', 'http', 'h2'].includes(form.transport_type)
  const handleNodeTypeChange = (event: ChangeEvent<HTMLSelectElement>) => {
    const value = event.target.value as NodeType
    setNodeType(value)
    setForm((prev) => ({ ...prev, ...nodeTypeDefaults(value) }))
  }

  // 字段级受控绑定,消掉每个输入框重复的 setForm 内联 handler;
  // 有联动逻辑的字段(端口/Reality 公钥/混淆类型)仍用自定义 handler
  const bindField = (field: StringField) => ({
    value: form[field],
    onChange: (event: ChangeEvent<HTMLInputElement | HTMLSelectElement>) => setForm((prev) => ({ ...prev, [field]: event.target.value })),
  })
  const bindCheckbox = (field: BooleanField) => ({
    checked: form[field],
    onChange: (event: ChangeEvent<HTMLInputElement>) => setForm((prev) => ({ ...prev, [field]: event.target.checked })),
  })

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
          <SelectField label="协议" options={NODE_TYPE_OPTIONS} value={nodeType} onChange={handleNodeTypeChange} />
        </div>

      <div className="form-grid single">
        <TextField label="节点名称" placeholder="例如：我的节点" {...bindField('tag')} />
      </div>

      <div className="form-grid two">
        <TextField label="服务器地址" placeholder="example.com" {...bindField('server')} />
        <TextField
          label="端口"
          type="number"
          min="1"
          max="65535"
          placeholder="443"
          value={form.server_port}
          onChange={(event: ChangeEvent<HTMLInputElement>) => setForm((prev) => ({
            ...prev,
            // 清空输入时保留空串（受控组件中间态），NodeForm 声明为 number，收窄仅在表单内成立
            server_port: (event.target.value === '' ? '' : Number(event.target.value)) as number,
          }))}
        />
      </div>

      {requiresPassword && (
        <div className="form-grid single">
          <TextField
            label="密码"
            type="password"
            autoComplete="new-password"
            placeholder="密码"
            {...bindField('password')}
          />
        </div>
      )}

      {requiresUuid && (
        <div className="form-grid single">
          <TextField label="UUID" placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" {...bindField('uuid')} />
        </div>
      )}

      {nodeType === 'ss' && (
        <div className="form-grid single">
          <SelectField label="加密方式" options={asOptions(CIPHER_OPTIONS)} {...bindField('cipher')} />
        </div>
      )}

      {showsTlsToggle && (
        <div className="form-grid single">
          <CheckboxField label="启用 TLS" {...bindCheckbox('tls_enabled')} />
        </div>
      )}

      <details className="advanced-details">
        <summary>高级选项</summary>
        <div className="advanced-details-body">
          {nodeType === 'vmess' && (
            <div className="form-grid two">
              <SelectField label="VMess security" options={asOptions(VMESS_CIPHER_OPTIONS)} {...bindField('vmess_cipher')} />
              <TextField
                label="Alter ID"
                type="number"
                min="0"
                value={form.alter_id}
                onChange={(event: ChangeEvent<HTMLInputElement>) => setForm((prev) => ({ ...prev, alter_id: Number(event.target.value || 0) }))}
              />
            </div>
          )}

          {nodeType === 'vless' && (
            <div className="form-grid two">
              <SelectField label="Flow" options={VLESS_FLOW_OPTIONS} {...bindField('flow')} />
              <SelectField label="Packet encoding" options={PACKET_ENCODING_OPTIONS} {...bindField('packet_encoding')} />
            </div>
          )}

          {nodeType === 'vmess' && (
            <div className="form-grid single">
              <SelectField label="Packet encoding" options={PACKET_ENCODING_OPTIONS} {...bindField('packet_encoding')} />
            </div>
          )}

          {showsTlsFields && (
            <>
              <div className="form-grid two">
                <TextField label="SNI（可选）" placeholder="留空使用服务器地址" {...bindField('sni')} />
                <SelectField label="TLS 指纹" options={CLIENT_FINGERPRINT_OPTIONS} {...bindField('client_fingerprint')} />
              </div>
              <div className="form-grid single">
                <CheckboxField label="跳过证书验证（不推荐）" {...bindCheckbox('skip_cert_verify')} />
              </div>
            </>
          )}

          {nodeType === 'vless' && (
            <div className="form-grid two">
              <TextField
                label="Reality public key"
                placeholder="可选"
                value={form.reality_public_key}
                onChange={(event: ChangeEvent<HTMLInputElement>) => {
                  const publicKey = event.target.value
                  setForm((prev) => ({
                    ...prev,
                    reality_public_key: publicKey,
                    client_fingerprint: publicKey.trim() && !prev.client_fingerprint
                      ? 'chrome'
                      : prev.client_fingerprint,
                  }))
                }}
              />
              <TextField label="Reality short ID" placeholder="可选" {...bindField('reality_short_id')} />
            </div>
          )}

          {supportsTransport && (
            <>
              <div className="form-grid single">
                <SelectField label="传输层" options={TRANSPORT_OPTIONS} {...bindField('transport_type')} />
              </div>
              {pathTransport && (
                <div className="form-grid two">
                  <TextField label="路径" placeholder="/ws" {...bindField('transport_path')} />
                  <TextField label="Host" placeholder="可选" {...bindField('transport_host')} />
                </div>
              )}
              {form.transport_type === 'grpc' && (
                <div className="form-grid single">
                  <TextField label="gRPC service name" placeholder="可选" {...bindField('grpc_service_name')} />
                </div>
              )}
            </>
          )}

          {nodeType === 'hysteria2' && (
            <div className="form-grid two">
              <SelectField
                label="混淆类型"
                options={HYSTERIA2_OBFS_OPTIONS}
                value={form.obfs_type}
                onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                  const obfsType = event.target.value
                  setForm((prev) => ({
                    ...prev,
                    obfs_type: obfsType,
                    obfs_password: obfsType ? prev.obfs_password : '',
                  }))
                }}
              />
              <TextField
                label="混淆密码"
                type="password"
                autoComplete="new-password"
                disabled={!form.obfs_type}
                placeholder={form.obfs_type ? 'obfs password' : '未启用'}
                {...bindField('obfs_password')}
              />
            </div>
          )}

          {nodeType === 'tuic' && (
            <div className="form-grid two">
              <SelectField label="拥塞控制" options={TUIC_CONGESTION_OPTIONS} {...bindField('tuic_congestion_control')} />
              <SelectField label="UDP relay mode" options={TUIC_UDP_RELAY_OPTIONS} {...bindField('tuic_udp_relay_mode')} />
            </div>
          )}

          {nodeType === 'tuic' && (
            <div className="form-grid single">
              <CheckboxField label="启用 0-RTT" {...bindCheckbox('tuic_zero_rtt')} />
            </div>
          )}
        </div>
      </details>
      </div>

      <Button
        tone="primary"
        loading={loading}
        icon={<Plus size={ICON.sm} />}
        disabled={!canSubmit || loading}
        onClick={onSubmit}
      >
        添加 {activeLabel} 节点
      </Button>
    </div>
  )
}
