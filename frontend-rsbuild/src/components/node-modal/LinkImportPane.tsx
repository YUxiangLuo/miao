import { For, Show, createMemo, createSignal } from 'solid-js'
import { ICON } from '../../tokens'
import { Button } from '../ui'
import { buildNodeRequest } from '../../nodeForm'
import { parseShareLinks, retainUnimportedShareLinks, type ParsedShareLink } from '../../shareLink'
import { EMPTY_NODE_FORM, NODE_TYPE_OPTIONS, nodeTypeDefaults } from '../../utils'
import type { BatchNodeResult, NodeRequest } from '../../types/api'
import { ListPlusIcon } from '../icons'

type LinkItem =
  | { line: string; ok: true; parsed: ParsedShareLink; payload: NodeRequest }
  | { line: string; ok: false; error: string }

function previewChips(parsed: ParsedShareLink): string[] {
  const patch = parsed.formPatch
  const chips: string[] = []
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

function ShareLinkItem(props: { item: LinkItem }) {
  const okItem = createMemo(() => (props.item.ok ? props.item : null))
  return (
    <Show
      when={okItem()}
      fallback={
        <div class="share-link-item error">
          <span class="share-link-item-line" title={props.item.line}>{props.item.line}</span>
          <span class="share-link-item-error">{!props.item.ok ? props.item.error : ''}</span>
        </div>
      }
    >
      {(valid) => {
        const typeLabel = createMemo(() => (
          NODE_TYPE_OPTIONS.find((option) => option.value === valid().parsed.nodeType)?.label || valid().parsed.nodeType
        ))
        const chips = createMemo(() => previewChips(valid().parsed))
        return (
          <div class="share-link-item">
            <div class="share-link-item-top">
              <span class="share-link-type">{typeLabel()}</span>
              <strong class="share-link-tag" title={valid().parsed.tag}>{valid().parsed.tag}</strong>
              <span class="share-link-server num">{valid().parsed.formPatch.server}:{valid().parsed.formPatch.server_port}</span>
            </div>
            <Show when={chips().length > 0}>
              <div class="share-link-chips">
                <For each={chips()}>
                  {(chip) => <span class="share-link-chip">{chip}</span>}
                </For>
              </div>
            </Show>
          </div>
        )
      }}
    </Show>
  )
}

export interface LinkImportPaneProps {
  onImport: (payloads: NodeRequest[], closeOnSuccess: boolean) => Promise<BatchNodeResult | undefined>
  loading: boolean
}

export function LinkImportPane(props: LinkImportPaneProps) {
  const [text, setText] = createSignal('')
  const [importing, setImporting] = createSignal(false)
  let inFlight = false
  const [resultMessage, setResultMessage] = createSignal('')

  const items = createMemo<LinkItem[]>(() => parseShareLinks(text()).map((item): LinkItem => {
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
      return { line: item.line, ok: false, error: error instanceof Error ? error.message : String(error) }
    }
  }))

  const validPayloads = createMemo(() => (
    items().filter((item): item is Extract<LinkItem, { ok: true }> => item.ok).map((item) => item.payload)
  ))
  const busy = () => props.loading || importing()

  const handleImport = async () => {
    const payloads = validPayloads()
    if (!payloads.length || busy() || inFlight) return
    inFlight = true
    setImporting(true)
    setResultMessage('')
    try {
      const currentItems = items()
      const result = await props.onImport(payloads, currentItems.every((item) => item.ok))
      if (!result) return
      const added = new Set(result.added.map((item) => item.index))
      const remaining = retainUnimportedShareLinks(currentItems, added)
      setText(remaining.map((item) => item.line).join('\n'))
      setResultMessage([
        `本次已添加 ${result.added.length} 个节点；未成功的链接已保留，可修改后重试。`,
        ...result.failed.map((item) => `${item.tag}：${item.message}`),
      ].join('\n'))
    } finally {
      inFlight = false
      setImporting(false)
    }
  }

  return (
    <div class="node-pane">
      <textarea
        class="share-link-input"
        data-autofocus
        rows={5}
        value={text()}
        disabled={importing()}
        onInput={(event) => {
          setText(event.currentTarget.value)
          setResultMessage('')
        }}
        placeholder={'粘贴节点分享链接,每行一条\n支持 hysteria2 / hy2 / ss / socks5 / vmess / vless / trojan / tuic / anytls\nSOCKS5 也可填写 地址:端口:用户名:密码'}
        aria-label="节点分享链接"
      />

      <div class="share-link-preview node-pane-scroll">
        <Show when={resultMessage()}>
          {(msg) => <p class="share-link-result" role="status">{msg()}</p>}
        </Show>
        <Show
          when={items().length > 0}
          fallback={<div class="share-link-empty">粘贴后自动识别节点信息,支持一次粘贴多条</div>}
        >
          <For each={items()} keyed={false}>
            {(item) => <ShareLinkItem item={item()} />}
          </For>
        </Show>
      </div>

      <Button
        tone="primary"
        icon={<ListPlusIcon size={ICON.sm} />}
        loading={busy()}
        disabled={!validPayloads().length || busy()}
        onClick={handleImport}
      >
        {validPayloads().length > 0 ? `添加 ${validPayloads().length} 个节点` : '添加节点'}
      </Button>
    </div>
  )
}
