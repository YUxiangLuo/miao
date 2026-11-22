import { For, Show, createEffect, createMemo, createSignal, createUniqueId, flush } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { classNames } from '../utils'
import { CheckIcon, ChevronDownIcon, SearchIcon } from './icons'

export interface SelectOption {
  value: string
  label: string
  group?: string
  badge?: string
  tone?: 'default' | 'emerald' | 'cobalt' | 'amber' | 'crimson'
  disabled?: boolean
}

export interface SelectPopoverProps {
  value: string
  options: SelectOption[]
  onChange: (value: string) => void
  ariaLabel: string
  placeholder?: string
  disabled?: boolean
  searchableThreshold?: number
  searchPlaceholder?: string
  align?: 'left' | 'right'
  size?: 'sm' | 'md'
  fullWidth?: boolean
  class?: string
}

const REGIONAL_PAIR_RE = /([\u{1F1E6}-\u{1F1FF}])([\u{1F1E6}-\u{1F1FF}])/u
const PICTOGRAPH_STRIP_RE = /(?:[\u{1F1E6}-\u{1F1FF}]{2}|[\u{1F1E6}-\u{1F1FF}]|[\u{2600}-\u{27BF}]|[\u{1F300}-\u{1FAFF}]|\uFE0F|\u200D|\u20E3)+/gu
const LEADING_PICTOGRAPH_RE = /^\s*(?:[\u{1F1E6}-\u{1F1FF}]|[\u{2600}-\u{27BF}]|[\u{1F300}-\u{1FAFF}])/u

export function cleanNodeDisplay(tag: string): { code: string | null; name: string } {
  const trimmed = tag.trim()
  if (!trimmed) return { code: null, name: '' }

  let detectedCode: string | null = null
  const flagMatch = REGIONAL_PAIR_RE.exec(trimmed)
  if (flagMatch) {
    const first = (flagMatch[1].codePointAt(0) ?? 0) - 0x1F1E6 + 65
    const second = (flagMatch[2].codePointAt(0) ?? 0) - 0x1F1E6 + 65
    if (first >= 65 && first <= 90 && second >= 65 && second <= 90) {
      detectedCode = String.fromCharCode(first, second)
    }
  }

  const hadLeadingPictograph = LEADING_PICTOGRAPH_RE.test(trimmed)
  let cleaned = trimmed.replace(PICTOGRAPH_STRIP_RE, ' ').replace(/\s+/g, ' ').trim()
  if (hadLeadingPictograph) {
    cleaned = cleaned.replace(/^[-·•|:]+\s*/, '').trim()
  }

  if (!cleaned) {
    return { code: null, name: trimmed }
  }

  return { code: detectedCode, name: cleaned }
}

interface FilteredItem {
  option: SelectOption
  display: { code: string | null; name: string }
  showGroupHeader: boolean
}

export function SelectPopover(props: SelectPopoverProps): JSX.Element {
  const listboxId = createUniqueId()
  const [open, setOpen] = createSignal(false)
  const [query, setQuery] = createSignal('')
  const [focusedIndex, setFocusedIndex] = createSignal(-1)
  const [flipUp, setFlipUp] = createSignal(false)

  let rootEl: HTMLDivElement | undefined
  let triggerEl: HTMLButtonElement | undefined
  let searchInputEl: HTMLInputElement | undefined
  let popoverEl: HTMLDivElement | undefined

  const searchable = createMemo(() => props.options.length > (props.searchableThreshold ?? 6))

  const selectedOption = createMemo(() => props.options.find((opt) => opt.value === props.value))

  const selectedDisplay = createMemo(() => {
    const opt = selectedOption()
    if (opt) return cleanNodeDisplay(opt.label)
    if (props.value) return cleanNodeDisplay(props.value)
    return { code: null, name: props.placeholder ?? '请选择' }
  })

  const filteredItems = createMemo<FilteredItem[]>(() => {
    const needle = query().trim().toLowerCase()
    const items: FilteredItem[] = []
    let lastGroup: string | undefined

    for (const option of props.options) {
      const display = cleanNodeDisplay(option.label)
      if (needle) {
        const haystack = [
          option.label,
          display.name,
          display.code ?? '',
          option.value,
          option.group ?? '',
          option.badge ?? '',
        ]
          .join(' ')
          .toLowerCase()
        if (!haystack.includes(needle)) continue
      }

      const showGroupHeader = Boolean(option.group && option.group !== lastGroup)
      if (option.group) lastGroup = option.group
      items.push({ option, display, showGroupHeader })
    }

    return items
  })

  const findInitialFocusIndex = (items: FilteredItem[]): number => {
    const selectedIdx = items.findIndex((item) => item.option.value === props.value && !item.option.disabled)
    if (selectedIdx >= 0) return selectedIdx
    return items.findIndex((item) => !item.option.disabled)
  }

  const openPopover = () => {
    if (props.disabled) return
    if (triggerEl && typeof window !== 'undefined') {
      const rect = triggerEl.getBoundingClientRect()
      const spaceBelow = window.innerHeight - rect.bottom
      setFlipUp(spaceBelow < 260 && rect.top > 260)
    }
    setQuery('')
    setOpen(true)
    setFocusedIndex(findInitialFocusIndex(filteredItems()))
  }

  const closePopover = (restoreFocus = false) => {
    setOpen(false)
    setQuery('')
    if (restoreFocus) triggerEl?.focus()
  }

  const togglePopover = () => {
    if (open()) closePopover(false)
    else openPopover()
  }

  const commitOption = (option: SelectOption) => {
    if (option.disabled) return
    props.onChange(option.value)
    closePopover(true)
  }

  const moveFocus = (delta: 1 | -1) => {
    const items = filteredItems()
    if (items.length === 0) return
    const total = items.length
    let idx = focusedIndex()
    for (let step = 0; step < total; step++) {
      idx = idx < 0 ? (delta > 0 ? 0 : total - 1) : (idx + delta + total) % total
      if (!items[idx].option.disabled) {
        setFocusedIndex(idx)
        const optionNodes = popoverEl?.querySelectorAll<HTMLElement>('.picker-option')
        const activeEl = optionNodes?.[idx]
        if (activeEl && typeof activeEl.scrollIntoView === 'function') {
          activeEl.scrollIntoView({ block: 'nearest' })
        }
        return
      }
    }
  }

  createEffect(
    () => Boolean(props.disabled && open()),
    (shouldClose) => {
      if (shouldClose) setOpen(false)
    },
  )

  createEffect(
    () => ({ isOpen: open(), isSearchable: searchable() }),
    ({ isOpen, isSearchable }) => {
      if (!isOpen) return

      let active = true
      queueMicrotask(() => {
        if (!active) return
        if (isSearchable && searchInputEl) {
          searchInputEl.focus()
        } else {
          popoverEl?.focus()
        }
      })

      const handlePointerDown = (event: PointerEvent | MouseEvent) => {
        const target = event.target
        if (rootEl && target instanceof Node && !rootEl.contains(target)) {
          closePopover(false)
          flush()
        }
      }

      document.addEventListener('pointerdown', handlePointerDown)
      return () => {
        active = false
        document.removeEventListener('pointerdown', handlePointerDown)
      }
    },
  )

  const handleRootKeyDown = (event: KeyboardEvent) => {
    if (!open()) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        openPopover()
      }
      return
    }

    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      closePopover(true)
      return
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault()
      moveFocus(1)
      return
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveFocus(-1)
      return
    }

    if (event.key === 'Enter') {
      event.preventDefault()
      event.stopPropagation()
      const current = filteredItems()[focusedIndex()]
      if (current && !current.option.disabled) {
        commitOption(current.option)
      }
    }
  }

  return (
    <div
      ref={(el) => { rootEl = el }}
      class={classNames('picker-root', props.fullWidth && 'full-width', props.class)}
      onKeyDown={handleRootKeyDown}
    >
      <button
        ref={(el) => { triggerEl = el }}
        type="button"
        class={classNames('picker-trigger', props.size ?? 'md', open() && 'open')}
        aria-label={props.ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open() ? 'true' : 'false'}
        aria-controls={open() ? listboxId : undefined}
        disabled={props.disabled}
        onClick={togglePopover}
      >
        <span class="picker-trigger-main">
          <Show when={selectedOption()?.tone && selectedOption()!.tone !== 'default'}>
            <span class={classNames('status-dot', selectedOption()!.tone)} aria-hidden="true" />
          </Show>
          <Show when={selectedDisplay().code}>
            {(code) => <span class="mono-tag">{code()}</span>}
          </Show>
          <span class="picker-trigger-label">{selectedDisplay().name}</span>
        </span>
        <Show when={selectedOption()?.badge}>
          {(badge) => <span class="picker-option-badge">{badge()}</span>}
        </Show>
        <ChevronDownIcon size={14} />
      </button>

      <Show when={open()}>
        <div
          ref={(el) => { popoverEl = el }}
          id={listboxId}
          role="listbox"
          aria-label={props.ariaLabel}
          tabindex={-1}
          class={classNames(
            'picker-popover',
            props.align === 'right' ? 'align-right' : 'align-left',
            flipUp() && 'flip-up',
          )}
        >
          <Show when={searchable()}>
            <div class="picker-search">
              <SearchIcon size={13} />
              <input
                ref={(el) => { searchInputEl = el }}
                type="text"
                aria-label={props.searchPlaceholder ?? '搜索选项或节点'}
                placeholder={props.searchPlaceholder ?? '搜索节点或地区…'}
                value={query()}
                onInput={(event) => {
                  setQuery(event.currentTarget.value)
                  const nextItems = filteredItems()
                  const firstEnabled = nextItems.findIndex((item) => !item.option.disabled)
                  setFocusedIndex(firstEnabled)
                }}
              />
            </div>
          </Show>

          <div class="picker-list">
            <Show
              when={filteredItems().length > 0}
              fallback={<div class="picker-empty">无匹配选项</div>}
            >
              <For each={filteredItems()}>
                {(item, index) => {
                  const isSelected = () => item.option.value === props.value
                  const isFocused = () => focusedIndex() === index()
                  return (
                    <>
                      <Show when={item.showGroupHeader && item.option.group}>
                        {(group) => (
                          <div class="picker-group-label" role="presentation">
                            {group()}
                          </div>
                        )}
                      </Show>
                      <button
                        type="button"
                        role="option"
                        aria-selected={isSelected() ? 'true' : 'false'}
                        disabled={item.option.disabled}
                        class={classNames(
                          'picker-option',
                          isSelected() && 'selected',
                          isFocused() && 'focused',
                        )}
                        onMouseEnter={() => {
                          if (!item.option.disabled) setFocusedIndex(index())
                        }}
                        onClick={() => commitOption(item.option)}
                      >
                        <span class="picker-option-main">
                          <Show when={item.option.tone && item.option.tone !== 'default'}>
                            <span class={classNames('status-dot', item.option.tone)} aria-hidden="true" />
                          </Show>
                          <Show when={item.display.code}>
                            {(code) => <span class="mono-tag">{code()}</span>}
                          </Show>
                          <span class="picker-trigger-label">{item.display.name}</span>
                        </span>
                        <Show when={item.option.badge || isSelected()}>
                          <span class="picker-option-badge">
                            {item.option.badge ?? ''}
                            <Show when={isSelected()}>
                              <CheckIcon size={12} />
                            </Show>
                          </span>
                        </Show>
                      </button>
                    </>
                  )
                }}
              </For>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  )
}
