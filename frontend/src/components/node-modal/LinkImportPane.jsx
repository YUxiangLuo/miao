import { useMemo, useState } from 'react'
import { ListPlus } from 'lucide-react'
import { Button } from '../ui.jsx'
import { buildNodeRequest } from '../../nodeForm.js'
import { parseShareLinks } from '../../shareLink.js'
import { EMPTY_NODE_FORM, NODE_TYPE_OPTIONS, nodeTypeDefaults } from '../../utils.js'

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

export function LinkImportPane({ onImport, loading }) {
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
