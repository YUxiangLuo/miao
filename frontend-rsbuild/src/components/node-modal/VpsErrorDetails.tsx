import { useRef, useState } from 'react'

function CommandBlock({ command }: { command: string }) {
  const container = useRef<HTMLDivElement>(null)
  const [copyState, setCopyState] = useState('复制命令')
  const copy = async () => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(command)
      } else {
        // Keep the fallback inside the dialog so its focus trap permits selection.
        const field = document.createElement('textarea')
        const focused = document.activeElement
        field.value = command
        field.className = 'vps-copy-buffer'
        container.current?.appendChild(field)
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
    <div className="vps-command" ref={container}>
      <button type="button" className="vps-command-copy" onClick={copy}>{copyState}</button>
      <pre><code>{command}</code></pre>
    </div>
  )
}

// Only render fenced commands in our help section. SSH output stays plain text.
export function VpsErrorDetails({ text }: { text: string }) {
  const rawIndex = text.indexOf('错误详情（退出状态 ')
  const help = rawIndex < 0 ? text : text.slice(0, rawIndex)
  const raw = rawIndex < 0 ? '' : text.slice(rawIndex)
  const blocks = help.split(/```sh\n([\s\S]*?)\n```/g)
  return (
    <>
      {blocks.map((block, index) => index % 2 === 1
        ? <CommandBlock key={index} command={block} />
        : block.trim() ? <div key={index} className="vps-deploy-error-message">{block.trim()}</div> : null)}
      {raw ? <div className="vps-deploy-error-message">{raw}</div> : null}
    </>
  )
}
