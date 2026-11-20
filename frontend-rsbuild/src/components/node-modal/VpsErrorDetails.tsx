import { For, Show, createMemo, createSignal } from 'solid-js'

function CommandBlock(props: { command: string }) {
  let container: HTMLDivElement | undefined
  const [copyState, setCopyState] = createSignal('复制命令')

  const copy = async () => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(props.command)
      } else {
        const field = document.createElement('textarea')
        const focused = document.activeElement
        field.value = props.command
        field.className = 'vps-copy-buffer'
        container?.appendChild(field)
        try {
          field.select()
          if (!document.execCommand('copy')) throw new Error('copy failed')
        } finally {
          field.remove()
          if (focused instanceof HTMLElement) focused.focus()
        }
      }
      setCopyState('已复制')
    } catch {
      setCopyState('复制失败，请选中命令手动复制')
    }
  }

  return (
    <div class="vps-command" ref={container}>
      <button type="button" class="vps-command-copy" onClick={copy}>{copyState()}</button>
      <pre><code>{props.command}</code></pre>
    </div>
  )
}

export function VpsErrorDetails(props: { text: string }) {
  const parsed = createMemo(() => {
    const rawIndex = props.text.indexOf('错误详情（退出状态 ')
    const help = rawIndex < 0 ? props.text : props.text.slice(0, rawIndex)
    const raw = rawIndex < 0 ? '' : props.text.slice(rawIndex)
    const blocks = help.split(/```sh\n([\s\S]*?)\n```/g)
    return { blocks, raw }
  })

  return (
    <>
      <For each={parsed().blocks}>
        {(block, index) => (
          <Show
            when={index() % 2 === 1}
            fallback={
              <Show when={block.trim()}>
                <div class="vps-deploy-error-message">{block.trim()}</div>
              </Show>
            }
          >
            <CommandBlock command={block} />
          </Show>
        )}
      </For>
      <Show when={parsed().raw}>
        {(raw) => <div class="vps-deploy-error-message">{raw()}</div>}
      </Show>
    </>
  )
}
