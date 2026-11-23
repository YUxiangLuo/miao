import { Show, createMemo } from 'solid-js'
import { iconForDomain } from './siteIcons'

export function AnimatedValue(props: { value: string | number; class?: string }) {
  return <span class={['value-anim', 'num', props.class]}>{props.value}</span>
}

export function SiteMark(props: { domain: string }) {
  const icon = createMemo(() => iconForDomain(props.domain))
  const isLetter = () => 'letter' in icon()

  return (
    <div
      class={['site-icon', { letter: isLetter() }]}
      style={!isLetter()
        ? {
          background: (icon() as { background: string }).background,
          color: (icon() as { color: string }).color,
        }
        : undefined}
      data-site={icon().id}
      title={icon().label}
      aria-hidden="true"
    >
      <Show
        when={!isLetter()}
        fallback={<span>{(icon() as { letter: string }).letter}</span>}
      >
        <svg viewBox={(icon() as { viewBox?: string }).viewBox || '0 0 24 24'} width="18" height="18">
          <path fill="currentColor" d={(icon() as { path: string }).path} />
        </svg>
      </Show>
    </div>
  )
}
