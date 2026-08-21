import { describe, expect, it } from '@rstest/core'
import { parseShareLink, parseShareLinks, sanitizeTag } from './shareLink'

function encodeBase64(input: string): string {
  const bytes = new TextEncoder().encode(input)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary)
}

describe('sanitizeTag', () => {
  it('strips characters the backend rejects', () => {
    expect(sanitizeTag('🇭🇰 HK-01 香港')).toBe('HK-01 香港')
    expect(sanitizeTag('  <script> 节点 ')).toBe('script 节点')
    expect(sanitizeTag('')).toBe('')
    expect(sanitizeTag('🇯🇵')).toBe('')
  })
})

describe('parseShareLink', () => {
  it('parses hysteria2 links with all options', () => {
    const parsed = parseShareLink(
      'hysteria2://pass123@example.com:443?sni=mask.com&insecure=1&obfs=salamander&obfs-password=obfs-pass#我的节点',
    )

    expect(parsed.nodeType).toBe('hysteria2')
    expect(parsed.tag).toBe('我的节点')
    expect(parsed.formPatch).toMatchObject({
      server: 'example.com',
      server_port: 443,
      password: 'pass123',
      sni: 'mask.com',
      skip_cert_verify: true,
      tls_enabled: true,
      obfs_type: 'salamander',
      obfs_password: 'obfs-pass',
    })
  })

  it('parses hy2 alias with defaults', () => {
    const parsed = parseShareLink('hy2://secret@1.2.3.4:8443')

    expect(parsed.nodeType).toBe('hysteria2')
    expect(parsed.formPatch).toMatchObject({
      server: '1.2.3.4',
      server_port: 8443,
      password: 'secret',
      skip_cert_verify: false,
      sni: '',
    })
    expect(parsed.tag).toBe('hysteria2 节点')
  })

  it('parses SIP002 ss links', () => {
    const userinfo = encodeBase64('2022-blake3-aes-128-gcm:pass123')
    const parsed = parseShareLink(`ss://${userinfo}@ss.example.com:8388#ss 节点`)

    expect(parsed.nodeType).toBe('ss')
    expect(parsed.tag).toBe('ss 节点')
    expect(parsed.formPatch).toMatchObject({
      server: 'ss.example.com',
      server_port: 8388,
      cipher: '2022-blake3-aes-128-gcm',
      password: 'pass123',
    })
  })

  it('parses legacy fully-encoded ss links', () => {
    const body = encodeBase64('aes-128-gcm:pass123@ss.example.com:8388')
    const parsed = parseShareLink(`ss://${body}#legacy`)

    expect(parsed.nodeType).toBe('ss')
    expect(parsed.formPatch).toMatchObject({
      server: 'ss.example.com',
      server_port: 8388,
      cipher: 'aes-128-gcm',
      password: 'pass123',
    })
  })

  it('rejects ss links with plugins', () => {
    const body = encodeBase64('aes-128-gcm:pass123@ss.example.com:8388')
    expect(() => parseShareLink(`ss://${body}?plugin=obfs-local#x`)).toThrow('暂不支持带插件')
  })

  it('keeps literal percent signs in base64-encoded ss passwords', () => {
    // base64 userinfo 里的密码是字面量,再做 percent-decoding 会抛 URIError 或改错密码
    const userinfo = encodeBase64('aes-128-gcm:100%secret')
    const parsed = parseShareLink(`ss://${userinfo}@ss.example.com:8388`)

    expect(parsed.formPatch).toMatchObject({ cipher: 'aes-128-gcm', password: '100%secret' })
  })

  it('does not double-decode percent escapes in legacy ss passwords', () => {
    const body = encodeBase64('aes-128-gcm:a%2Fb%2Fcdef@ss.example.com:8388')
    const parsed = parseShareLink(`ss://${body}`)

    expect(parsed.formPatch.password).toBe('a%2Fb%2Fcdef')
  })

  it('decodes percent-encoded passwords in plaintext ss userinfo', () => {
    const parsed = parseShareLink('ss://aes-128-gcm:a%2Fb%2Fcdef@ss.example.com:8388')

    expect(parsed.formPatch.password).toBe('a/b/cdef')
  })

  it('parses ss links with a slash before the query or hash', () => {
    const userinfo = encodeBase64('aes-128-gcm:pass123')
    const withQuery = parseShareLink(`ss://${userinfo}@ss.example.com:8388/?unused=1#ss`)
    const withHash = parseShareLink(`ss://${userinfo}@ss.example.com:8388/#ss`)

    expect(withQuery.formPatch).toMatchObject({ server: 'ss.example.com', server_port: 8388 })
    expect(withHash.formPatch).toMatchObject({ server: 'ss.example.com', server_port: 8388 })
  })

  it('strips IPv6 brackets from ss servers', () => {
    const userinfo = encodeBase64('aes-128-gcm:pass123')
    const parsed = parseShareLink(`ss://${userinfo}@[2001:db8::1]:8388`)

    expect(parsed.formPatch).toMatchObject({ server: '2001:db8::1', server_port: 8388 })
  })

  it('parses vmess links from base64 JSON', () => {
    const json = JSON.stringify({
      ps: 'vmess 节点',
      add: 'vm.example.com',
      port: '443',
      id: '123e4567-e89b-12d3-a456-426614174000',
      aid: '0',
      scy: 'auto',
      net: 'ws',
      path: '/ws',
      host: 'cdn.example.com',
      tls: 'tls',
      sni: 'sni.example.com',
    })
    const parsed = parseShareLink(`vmess://${encodeBase64(json)}`)

    expect(parsed.nodeType).toBe('vmess')
    expect(parsed.tag).toBe('vmess 节点')
    expect(parsed.formPatch).toMatchObject({
      server: 'vm.example.com',
      server_port: 443,
      uuid: '123e4567-e89b-12d3-a456-426614174000',
      alter_id: 0,
      vmess_cipher: 'auto',
      tls_enabled: true,
      sni: 'sni.example.com',
      transport_type: 'ws',
      transport_path: '/ws',
      transport_host: 'cdn.example.com',
    })
  })

  it('parses vless reality links with ws transport', () => {
    const parsed = parseShareLink(
      'vless://123e4567-e89b-12d3-a456-426614174000@vl.example.com:443?security=reality&flow=xtls-rprx-vision&sni=www.microsoft.com&fp=chrome&pbk=PUBKEY&sid=ab12&type=ws&path=%2Fws&host=cdn.com#vless 节点',
    )

    expect(parsed.nodeType).toBe('vless')
    expect(parsed.tag).toBe('vless 节点')
    expect(parsed.formPatch).toMatchObject({
      server: 'vl.example.com',
      server_port: 443,
      uuid: '123e4567-e89b-12d3-a456-426614174000',
      tls_enabled: true,
      flow: 'xtls-rprx-vision',
      sni: 'www.microsoft.com',
      client_fingerprint: 'chrome',
      reality_public_key: 'PUBKEY',
      reality_short_id: 'ab12',
      transport_type: 'ws',
      transport_path: '/ws',
      transport_host: 'cdn.com',
    })
  })

  it('parses trojan links', () => {
    const parsed = parseShareLink('trojan://pass123@tj.example.com:443?sni=tj.example.com#trojan')

    expect(parsed.nodeType).toBe('trojan')
    expect(parsed.tag).toBe('trojan')
    expect(parsed.formPatch).toMatchObject({
      server: 'tj.example.com',
      server_port: 443,
      password: 'pass123',
      sni: 'tj.example.com',
      tls_enabled: true,
      transport_type: 'tcp',
    })
  })

  it('parses tuic links', () => {
    const parsed = parseShareLink(
      'tuic://123e4567-e89b-12d3-a456-426614174000:pass123@tuic.example.com:443?congestion_control=bbr&udp_relay_mode=quic&sni=tuic.example.com#tuic 节点',
    )

    expect(parsed.nodeType).toBe('tuic')
    expect(parsed.formPatch).toMatchObject({
      server: 'tuic.example.com',
      server_port: 443,
      uuid: '123e4567-e89b-12d3-a456-426614174000',
      password: 'pass123',
      sni: 'tuic.example.com',
      tuic_congestion_control: 'bbr',
      tuic_udp_relay_mode: 'quic',
    })
  })

  it('parses anytls links', () => {
    const parsed = parseShareLink('anytls://pass123@any.example.com:443?insecure=1#any')

    expect(parsed.nodeType).toBe('anytls')
    expect(parsed.formPatch).toMatchObject({
      server: 'any.example.com',
      server_port: 443,
      password: 'pass123',
      skip_cert_verify: true,
      tls_enabled: true,
    })
  })

  it('rejects unsupported schemes and malformed links', () => {
    expect(() => parseShareLink('http://example.com')).toThrow('不支持的链接类型')
    expect(() => parseShareLink('not-a-link')).toThrow('不支持的链接类型')
    expect(() => parseShareLink('hysteria2://pass@example.com')).toThrow('缺少有效端口')
    expect(() => parseShareLink('vmess://!!!not-base64!!!')).toThrow('VMess 链接解析失败')
  })
})

describe('parseShareLinks', () => {
  it('parses multiple lines, skipping blanks and collecting errors', () => {
    const results = parseShareLinks(`
      hysteria2://pass123@a.example.com:443#节点一

      invalid-line
      trojan://pass123@b.example.com:443#节点二
    `)

    expect(results).toHaveLength(3)
    const [first, second, third] = results
    if (!first?.ok) throw new Error('expected first link to parse')
    expect(first.parsed.tag).toBe('节点一')
    if (second?.ok) throw new Error('expected second link to fail')
    expect(second.error).toBeTruthy()
    if (!third?.ok) throw new Error('expected third link to parse')
    expect(third.parsed.nodeType).toBe('trojan')
  })
})
