import {
  buildTransportPayload,
  nodeCapabilities,
  validateHysteria2Obfs,
  validateNodeTag,
  validatePassword,
  validatePort,
  validateServer,
  validateTransport,
  validateUuid,
  validateVlessFlow,
  type NodeForm,
} from './utils'
import type { NodeRequest, NodeType } from './types/api'

const UTF8_ENCODER = new TextEncoder()
const MAX_SOCKS_CREDENTIAL_BYTES = 255

function assertValid(message: string | null): void {
  if (message) throw new Error(message)
}

function validateSocksCredentials(username: string, password: string): void {
  if (Boolean(username) !== Boolean(password)) {
    throw new Error('SOCKS5 用户名和密码须同时填写，或同时留空')
  }
  if (
    UTF8_ENCODER.encode(username).length > MAX_SOCKS_CREDENTIAL_BYTES
    || UTF8_ENCODER.encode(password).length > MAX_SOCKS_CREDENTIAL_BYTES
  ) {
    throw new Error('SOCKS5 用户名和密码各不能超过 255 字节')
  }
}

function validateVlessOptions(form: NodeForm): void {
  assertValid(validateVlessFlow(form.flow))
  const hasRealityConfig = Boolean(form.reality_public_key?.trim() || form.reality_short_id?.trim())
  if (hasRealityConfig && !form.client_fingerprint?.trim()) {
    throw new Error('Reality 节点必须配置 TLS 指纹（uTLS）')
  }
}

export function validateNodeForm(nodeType: NodeType, form: NodeForm): void {
  const caps = nodeCapabilities(nodeType)

  assertValid(validateNodeTag(form.tag.trim()))
  assertValid(validateServer(form.server.trim()))
  assertValid(validatePort(form.server_port))

  const detour = form.detour?.trim() || ''
  if (detour) {
    if (detour.length > 128) throw new Error('前置中转节点名称不能超过 128 个字符')
    if (['proxy', 'direct', 'reject'].includes(detour.toLowerCase())) {
      throw new Error('前置中转节点不能使用保留字 proxy / direct / reject')
    }
    if (detour.toLowerCase() === form.tag.trim().toLowerCase()) {
      throw new Error('前置中转节点不能指向自身')
    }
  }

  if (caps.password) assertValid(validatePassword(form.password.trim()))
  if (caps.uuid) assertValid(validateUuid(form.uuid))
  if (caps.transport) {
    assertValid(validateTransport(
      form.transport_type,
      form.transport_path,
      form.transport_host,
      form.grpc_service_name,
    ))
  }

  switch (nodeType) {
    case 'socks':
      validateSocksCredentials(form.username, form.password)
      break
    case 'vless':
      validateVlessOptions(form)
      break
    case 'hysteria2':
      assertValid(validateHysteria2Obfs(form.obfs_type, form.obfs_password))
      break
    case 'anytls':
    case 'ss':
    case 'vmess':
    case 'trojan':
    case 'tuic':
      break
  }
}

function buildCommonTlsFields(
  form: Pick<NodeForm, 'sni' | 'skip_cert_verify' | 'client_fingerprint'>,
): Pick<NodeRequest, 'sni' | 'skip_cert_verify' | 'client_fingerprint'> {
  const tls: Pick<NodeRequest, 'sni' | 'skip_cert_verify' | 'client_fingerprint'> = {
    skip_cert_verify: form.skip_cert_verify,
  }
  if (form.sni?.trim()) tls.sni = form.sni.trim()
  if (form.client_fingerprint?.trim()) tls.client_fingerprint = form.client_fingerprint.trim()
  return tls
}

export function buildNodeRequest(nodeType: NodeType, form: NodeForm): NodeRequest {
  validateNodeForm(nodeType, form)

  const detour = form.detour?.trim()
  const base: NodeRequest = {
    node_type: nodeType,
    tag: form.tag.trim(),
    server: form.server.trim(),
    server_port: form.server_port,
    ...(detour ? { detour } : {}),
  }

  switch (nodeType) {
    case 'socks':
      // SOCKS5 credentials are preserved verbatim (no trimming) and exclude TLS/transport fields.
      return form.username
        ? { ...base, username: form.username, password: form.password }
        : base

    case 'ss':
      return {
        ...base,
        password: form.password.trim(),
        cipher: form.cipher,
      }

    case 'anytls':
      return {
        ...base,
        password: form.password.trim(),
        ...buildCommonTlsFields(form),
      }

    case 'hysteria2':
      return {
        ...base,
        password: form.password.trim(),
        ...buildCommonTlsFields(form),
        ...(form.obfs_type ? { obfs_type: form.obfs_type, obfs_password: form.obfs_password.trim() } : {}),
      }

    case 'trojan':
      return {
        ...base,
        password: form.password.trim(),
        ...buildCommonTlsFields(form),
        ...buildTransportPayload(form),
      }

    case 'tuic':
      return {
        ...base,
        password: form.password.trim(),
        uuid: form.uuid.trim(),
        ...buildCommonTlsFields(form),
        tuic_congestion_control: form.tuic_congestion_control,
        tuic_udp_relay_mode: form.tuic_udp_relay_mode,
        tuic_zero_rtt: Boolean(form.tuic_zero_rtt),
      }

    case 'vmess':
      return {
        ...base,
        uuid: form.uuid.trim(),
        ...buildCommonTlsFields(form),
        cipher: form.vmess_cipher,
        alter_id: Number(form.alter_id || 0),
        tls_enabled: Boolean(form.tls_enabled),
        ...(form.packet_encoding ? { packet_encoding: form.packet_encoding } : {}),
        ...buildTransportPayload(form),
      }

    case 'vless':
      return {
        ...base,
        uuid: form.uuid.trim(),
        ...buildCommonTlsFields(form),
        tls_enabled: Boolean(form.tls_enabled),
        ...(form.flow ? { flow: form.flow } : {}),
        ...(form.packet_encoding ? { packet_encoding: form.packet_encoding } : {}),
        ...(form.reality_public_key?.trim() ? { reality_public_key: form.reality_public_key.trim() } : {}),
        ...(form.reality_short_id?.trim() ? { reality_short_id: form.reality_short_id.trim() } : {}),
        ...buildTransportPayload(form),
      }
  }
}
