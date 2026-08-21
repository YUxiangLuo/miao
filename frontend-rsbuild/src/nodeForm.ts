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

function assertValid(message: string | null): void {
  if (message) throw new Error(message)
}

export function buildNodeRequest(nodeType: NodeType, form: NodeForm): NodeRequest {
  const caps = nodeCapabilities(nodeType)

  assertValid(validateNodeTag(form.tag.trim()))
  assertValid(validateServer(form.server.trim()))
  assertValid(validatePort(form.server_port))

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

  if (nodeType === 'vless') {
    assertValid(validateVlessFlow(form.flow))
    const hasRealityConfig = form.reality_public_key?.trim() || form.reality_short_id?.trim()
    if (hasRealityConfig && !form.client_fingerprint?.trim()) {
      throw new Error('Reality 节点必须配置 TLS 指纹（uTLS）')
    }
  }

  if (nodeType === 'hysteria2') {
    assertValid(validateHysteria2Obfs(form.obfs_type, form.obfs_password))
  }

  const payload: NodeRequest = {
    node_type: nodeType,
    tag: form.tag.trim(),
    server: form.server.trim(),
    server_port: form.server_port,
  }

  if (caps.password) payload.password = form.password.trim()
  if (caps.uuid) payload.uuid = form.uuid.trim()

  if (nodeType === 'ss') {
    payload.cipher = form.cipher
  } else {
    if (form.sni?.trim()) payload.sni = form.sni.trim()
    payload.skip_cert_verify = form.skip_cert_verify
    if (form.client_fingerprint?.trim()) {
      payload.client_fingerprint = form.client_fingerprint.trim()
    }
    if (nodeType === 'hysteria2' && form.obfs_type) {
      payload.obfs_type = form.obfs_type
      payload.obfs_password = form.obfs_password.trim()
    }
  }

  if (nodeType === 'vmess') {
    payload.cipher = form.vmess_cipher
    payload.alter_id = Number(form.alter_id || 0)
    payload.tls_enabled = Boolean(form.tls_enabled)
    if (form.packet_encoding) payload.packet_encoding = form.packet_encoding
  }

  if (nodeType === 'vless') {
    payload.tls_enabled = Boolean(form.tls_enabled)
    if (form.flow) payload.flow = form.flow
    if (form.packet_encoding) payload.packet_encoding = form.packet_encoding
    if (form.reality_public_key?.trim()) {
      payload.reality_public_key = form.reality_public_key.trim()
    }
    if (form.reality_short_id?.trim()) {
      payload.reality_short_id = form.reality_short_id.trim()
    }
  }

  if (caps.transport) Object.assign(payload, buildTransportPayload(form))

  if (nodeType === 'tuic') {
    payload.tuic_congestion_control = form.tuic_congestion_control
    payload.tuic_udp_relay_mode = form.tuic_udp_relay_mode
    payload.tuic_zero_rtt = Boolean(form.tuic_zero_rtt)
  }

  return payload
}
