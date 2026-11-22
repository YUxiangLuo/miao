import { Show, createMemo, omit, type Setter } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { ICON } from '../../tokens'
import { Button } from '../ui'
import { SelectPopover, type SelectOption } from '../SelectPopover'
import { buildNodeRequest } from '../../nodeForm'
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
} from '../../utils'
import type { NodeType } from '../../types/api'
import { PlusIcon } from '../icons'

const VLESS_FLOW_OPTIONS: SelectOption[] = [
  { value: '', label: '默认' },
  { value: 'xtls-rprx-vision', label: 'xtls-rprx-vision' },
]

const asOptions = (items: string[]): SelectOption[] => items.map((item) => ({ value: item, label: item }))

interface TextFieldProps extends JSX.InputHTMLAttributes<HTMLInputElement> {
  label: string
}

function TextField(props: TextFieldProps) {
  const rest = omit(props, 'label')
  return (
    <label class="field">
      <span>{props.label}</span>
      <input {...rest} />
    </label>
  )
}

interface SelectFieldProps {
  label: string
  value: string
  options: SelectOption[]
  disabled?: boolean
  onChange: (value: string) => void
}

function SelectField(props: SelectFieldProps) {
  return (
    <div class="field">
      <span>{props.label}</span>
      <SelectPopover
        ariaLabel={props.label}
        value={props.value}
        options={props.options}
        disabled={props.disabled}
        fullWidth
        onChange={props.onChange}
      />
    </div>
  )
}

function CheckboxField(props: TextFieldProps) {
  const rest = omit(props, 'label', 'type')
  return (
    <label class="field checkbox-field">
      <input type="checkbox" {...rest} />
      <span>{props.label}</span>
    </label>
  )
}

type StringField = { [K in keyof NodeForm]: NodeForm[K] extends string ? K : never }[keyof NodeForm]
type BooleanField = { [K in keyof NodeForm]: NodeForm[K] extends boolean ? K : never }[keyof NodeForm]

export interface ManualPaneProps {
  nodeType: NodeType
  setNodeType: Setter<NodeType>
  form: NodeForm
  setForm: Setter<NodeForm>
  loading: boolean
  onSubmit: () => void
  relayCandidates?: string[]
}

export function ManualPane(props: ManualPaneProps) {
  const activeLabel = () => NODE_TYPE_OPTIONS.find((option) => option.value === props.nodeType)?.label || props.nodeType
  const caps = createMemo(() => nodeCapabilities(props.nodeType))
  const showsTlsFields = () => (
    !['ss', 'socks'].includes(props.nodeType)
    && (!caps().tlsToggle || props.form.tls_enabled || Boolean(props.form.reality_public_key.trim()))
  )
  const pathTransport = () => ['ws', 'http', 'h2'].includes(props.form.transport_type)

  const detourOptions = createMemo<SelectOption[]>(() => [
    { value: '', label: '直接连接（不使用前置跳板）' },
    ...(props.relayCandidates || [])
      .filter((name) => name !== props.form.tag.trim())
      .map((candidate) => ({ value: candidate, label: candidate })),
  ])

  const handleNodeTypeChange = (value: NodeType) => {
    props.setNodeType(value)
    props.setForm((prev) => ({ ...prev, ...nodeTypeDefaults(value) }))
  }

  const updateString = (field: StringField, value: string) => {
    props.setForm((prev) => ({ ...prev, [field]: value }))
  }

  const updateBoolean = (field: BooleanField, checked: boolean) => {
    props.setForm((prev) => ({ ...prev, [field]: checked }))
  }

  const hasRequiredFields = createMemo(() => Boolean(
    props.form.tag.trim()
    && props.form.server.trim()
    && String(props.form.server_port).length > 0
    && (!caps().password || props.form.password.trim())
    && (!caps().uuid || props.form.uuid.trim())
    && (props.nodeType !== 'hysteria2' || !props.form.obfs_type || props.form.obfs_password.trim()),
  ))

  const validationError = createMemo(() => {
    if (!hasRequiredFields()) return ''
    try {
      buildNodeRequest(props.nodeType, props.form)
      return ''
    } catch (error) {
      return error instanceof Error ? error.message : String(error)
    }
  })

  const canSubmit = () => hasRequiredFields() && !validationError()

  return (
    <div class="node-pane">
      <fieldset class="node-pane-scroll manual-node-fields" disabled={props.loading}>
        <div class="form-grid single">
          <SelectField
            label="协议"
            options={NODE_TYPE_OPTIONS}
            value={props.nodeType}
            disabled={props.loading}
            onChange={(value) => handleNodeTypeChange(value as NodeType)}
          />
        </div>

        <div class="form-grid single">
          <TextField
            label="节点名称"
            placeholder="例如：我的节点"
            value={props.form.tag}
            onInput={(event) => updateString('tag', event.currentTarget.value)}
          />
        </div>

        <div class="form-grid two">
          <TextField
            label="服务器地址"
            placeholder="example.com"
            value={props.form.server}
            onInput={(event) => updateString('server', event.currentTarget.value)}
          />
          <TextField
            label="端口"
            type="number"
            min="1"
            max="65535"
            placeholder="443"
            value={props.form.server_port}
            onInput={(event) => props.setForm((prev) => ({
              ...prev,
              server_port: (event.currentTarget.value === '' ? '' : Number(event.currentTarget.value)) as number,
            }))}
          />
        </div>

        <Show when={props.nodeType === 'socks'}>
          <div class="form-grid single">
            <TextField
              label="用户名（可选）"
              autocomplete="off"
              placeholder="无认证时与密码一起留空"
              value={props.form.username}
              onInput={(event) => updateString('username', event.currentTarget.value)}
            />
          </div>
        </Show>

        <Show when={caps().password || props.nodeType === 'socks'}>
          <div class="form-grid single">
            <TextField
              label={props.nodeType === 'socks' ? '密码（可选）' : '密码'}
              type="password"
              autocomplete="new-password"
              placeholder="密码"
              value={props.form.password}
              onInput={(event) => updateString('password', event.currentTarget.value)}
            />
          </div>
        </Show>

        <div class="form-grid single">
          <div class="field">
            <span>前置跳板节点（可选）</span>
            <SelectPopover
              ariaLabel="前置跳板中转"
              value={props.form.detour}
              options={detourOptions()}
              disabled={props.loading}
              searchPlaceholder="搜索跳板节点…"
              fullWidth
              onChange={(value) => updateString('detour', value)}
            />
          </div>
        </div>

        <Show when={caps().uuid}>
          <div class="form-grid single">
            <TextField
              label="UUID"
              placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
              value={props.form.uuid}
              onInput={(event) => updateString('uuid', event.currentTarget.value)}
            />
          </div>
        </Show>

        <Show when={props.nodeType === 'ss'}>
          <div class="form-grid single">
            <SelectField
              label="加密方式"
              options={asOptions(CIPHER_OPTIONS)}
              value={props.form.cipher}
              disabled={props.loading}
              onChange={(value) => updateString('cipher', value)}
            />
          </div>
        </Show>

        <Show when={caps().tlsToggle}>
          <div class="form-grid single">
            <CheckboxField
              label="启用 TLS"
              checked={props.form.tls_enabled}
              onChange={(event) => updateBoolean('tls_enabled', event.currentTarget.checked)}
            />
          </div>
        </Show>

        <Show when={props.nodeType !== 'socks'}>
          <details class="advanced-details">
            <summary>高级选项</summary>
            <div class="advanced-details-body">
              <Show when={props.nodeType === 'vmess'}>
                <div class="form-grid two">
                  <SelectField
                    label="VMess security"
                    options={asOptions(VMESS_CIPHER_OPTIONS)}
                    value={props.form.vmess_cipher}
                    disabled={props.loading}
                    onChange={(value) => updateString('vmess_cipher', value)}
                  />
                  <TextField
                    label="Alter ID"
                    type="number"
                    min="0"
                    value={props.form.alter_id}
                    onInput={(event) => props.setForm((prev) => ({
                      ...prev,
                      alter_id: Number(event.currentTarget.value || 0),
                    }))}
                  />
                </div>
              </Show>

              <Show when={props.nodeType === 'vless'}>
                <div class="form-grid two">
                  <SelectField
                    label="Flow"
                    options={VLESS_FLOW_OPTIONS}
                    value={props.form.flow}
                    disabled={props.loading}
                    onChange={(value) => updateString('flow', value)}
                  />
                  <SelectField
                    label="Packet encoding"
                    options={PACKET_ENCODING_OPTIONS}
                    value={props.form.packet_encoding}
                    disabled={props.loading}
                    onChange={(value) => updateString('packet_encoding', value)}
                  />
                </div>
              </Show>

              <Show when={props.nodeType === 'vmess'}>
                <div class="form-grid single">
                  <SelectField
                    label="Packet encoding"
                    options={PACKET_ENCODING_OPTIONS}
                    value={props.form.packet_encoding}
                    disabled={props.loading}
                    onChange={(value) => updateString('packet_encoding', value)}
                  />
                </div>
              </Show>

              <Show when={showsTlsFields()}>
                <div class="form-grid two">
                  <TextField
                    label="SNI（可选）"
                    placeholder="留空使用服务器地址"
                    value={props.form.sni}
                    onInput={(event) => updateString('sni', event.currentTarget.value)}
                  />
                  <SelectField
                    label="TLS 指纹"
                    options={CLIENT_FINGERPRINT_OPTIONS}
                    value={props.form.client_fingerprint}
                    disabled={props.loading}
                    onChange={(value) => updateString('client_fingerprint', value)}
                  />
                </div>
                <div class="form-grid single">
                  <CheckboxField
                    label="跳过证书验证（不推荐）"
                    checked={props.form.skip_cert_verify}
                    onChange={(event) => updateBoolean('skip_cert_verify', event.currentTarget.checked)}
                  />
                </div>
              </Show>

              <Show when={props.nodeType === 'vless'}>
                <div class="form-grid two">
                  <TextField
                    label="Reality public key"
                    placeholder="可选"
                    value={props.form.reality_public_key}
                    onInput={(event) => {
                      const publicKey = event.currentTarget.value
                      props.setForm((prev) => ({
                        ...prev,
                        reality_public_key: publicKey,
                        client_fingerprint: publicKey.trim() && !prev.client_fingerprint
                          ? 'chrome'
                          : prev.client_fingerprint,
                      }))
                    }}
                  />
                  <TextField
                    label="Reality short ID"
                    placeholder="可选"
                    value={props.form.reality_short_id}
                    onInput={(event) => updateString('reality_short_id', event.currentTarget.value)}
                  />
                </div>
              </Show>

              <Show when={caps().transport}>
                <div class="form-grid single">
                  <SelectField
                    label="传输层"
                    options={TRANSPORT_OPTIONS}
                    value={props.form.transport_type}
                    disabled={props.loading}
                    onChange={(value) => updateString('transport_type', value)}
                  />
                </div>
                <Show when={pathTransport()}>
                  <div class="form-grid two">
                    <TextField
                      label="路径"
                      placeholder="/ws"
                      value={props.form.transport_path}
                      onInput={(event) => updateString('transport_path', event.currentTarget.value)}
                    />
                    <TextField
                      label="Host"
                      placeholder="可选"
                      value={props.form.transport_host}
                      onInput={(event) => updateString('transport_host', event.currentTarget.value)}
                    />
                  </div>
                </Show>
                <Show when={props.form.transport_type === 'grpc'}>
                  <div class="form-grid single">
                    <TextField
                      label="gRPC service name"
                      placeholder="可选"
                      value={props.form.grpc_service_name}
                      onInput={(event) => updateString('grpc_service_name', event.currentTarget.value)}
                    />
                  </div>
                </Show>
              </Show>

              <Show when={props.nodeType === 'hysteria2'}>
                <div class="form-grid two">
                  <SelectField
                    label="混淆类型"
                    options={HYSTERIA2_OBFS_OPTIONS}
                    value={props.form.obfs_type}
                    disabled={props.loading}
                    onChange={(obfsType) => {
                      props.setForm((prev) => ({
                        ...prev,
                        obfs_type: obfsType,
                        obfs_password: obfsType ? prev.obfs_password : '',
                      }))
                    }}
                  />
                  <TextField
                    label="混淆密码"
                    type="password"
                    autocomplete="new-password"
                    disabled={!props.form.obfs_type}
                    placeholder={props.form.obfs_type ? 'obfs password' : '未启用'}
                    value={props.form.obfs_password}
                    onInput={(event) => updateString('obfs_password', event.currentTarget.value)}
                  />
                </div>
              </Show>

              <Show when={props.nodeType === 'tuic'}>
                <div class="form-grid two">
                  <SelectField
                    label="拥塞控制"
                    options={TUIC_CONGESTION_OPTIONS}
                    value={props.form.tuic_congestion_control}
                    disabled={props.loading}
                    onChange={(value) => updateString('tuic_congestion_control', value)}
                  />
                  <SelectField
                    label="UDP relay mode"
                    options={TUIC_UDP_RELAY_OPTIONS}
                    value={props.form.tuic_udp_relay_mode}
                    disabled={props.loading}
                    onChange={(value) => updateString('tuic_udp_relay_mode', value)}
                  />
                </div>
                <div class="form-grid single">
                  <CheckboxField
                    label="启用 0-RTT"
                    checked={props.form.tuic_zero_rtt}
                    onChange={(event) => updateBoolean('tuic_zero_rtt', event.currentTarget.checked)}
                  />
                </div>
              </Show>
            </div>
          </details>
        </Show>
      </fieldset>

      <Show when={validationError()}>
        {(err) => <p class="manual-node-error" role="alert">{err()}</p>}
      </Show>

      <Button
        tone="primary"
        loading={props.loading}
        icon={<PlusIcon size={ICON.sm} />}
        disabled={!canSubmit() || props.loading}
        onClick={props.onSubmit}
      >
        添加 {activeLabel()} 节点
      </Button>
    </div>
  )
}
