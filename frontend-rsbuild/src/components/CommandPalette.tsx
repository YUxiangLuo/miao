import { For, Show, createEffect, createMemo, createSignal, onSettled } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { formatDelay } from '../utils'
import type { useAppController } from '../hooks/useAppController'
import { cleanNodeDisplay } from './SelectPopover'
import { SearchIcon } from './icons'

export interface CommandPaletteProps {
  open: boolean
  onClose: () => void
  app: ReturnType<typeof useAppController>
  onToggleTheme: () => void
  theme: 'light' | 'dark'
}

type ControllerWithAliases = ReturnType<typeof useAppController> & {
  proxyNodes?: () => string[]
  handleSelectProxy?: (groupName: string, nodeName: string) => unknown
}

interface CommandEntry {
  id: string
  group: string
  label: string
  keywords: string
  code?: string | null
  badge?: string
  activeDot?: boolean
  run: () => void
}

interface CommandRenderRow {
  entry: CommandEntry
  showGroupTitle: boolean
}

export function useCommandShortcut(onToggle: () => void): void {
  onSettled(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        onToggle()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => {
      window.removeEventListener('keydown', handleKeyDown)
    }
  })
}

export function CommandPalette(props: CommandPaletteProps): JSX.Element {
  const [query, setQuery] = createSignal('')
  const [activeIndex, setActiveIndex] = createSignal(0)
  let inputEl: HTMLInputElement | undefined
  let listEl: HTMLDivElement | undefined

  const ctrl = () => props.app as ControllerWithAliases

  const sharedExitNodes = createMemo<string[]>(() => {
    const app = ctrl()
    if (!app.status().ready) return []
    if (typeof app.proxyNodes === 'function') return app.proxyNodes()
    return app.primaryGroup?.()?.all ?? []
  })

  const currentExitNode = createMemo(() => ctrl().primaryGroup?.()?.now ?? '')

  const switchSharedExit = (nodeName: string) => {
    const app = ctrl()
    const group = app.primaryGroupName?.() || 'proxy'
    if (typeof app.handleSelectProxy === 'function') {
      void app.handleSelectProxy(group, nodeName)
      return
    }
    const requested = app.status().requested_node_select || app.status().node_select || 'manual'
    if (requested !== 'manual' && typeof app.handleSetNodeSelect === 'function') {
      void app.handleSetNodeSelect('manual').then(() => app.handleSwitchProxy(group, nodeName))
      return
    }
    void app.handleSwitchProxy(group, nodeName)
  }

  const allCommands = createMemo<CommandEntry[]>(() => {
    const app = ctrl()
    const entries: CommandEntry[] = [
      {
        id: 'nav:overview',
        group: '页面导航',
        label: '前往 概览',
        keywords: 'overview 概览 首页 控制台 总览',
        badge: app.activeNav() === 'overview' ? '当前' : undefined,
        run: () => app.setActiveNav('overview'),
      },
      {
        id: 'nav:nodes',
        group: '页面导航',
        label: '前往 节点与订阅',
        keywords: 'nodes 节点 订阅 测速 线路 subs proxies',
        badge: app.activeNav() === 'nodes' ? '当前' : undefined,
        run: () => app.setActiveNav('nodes'),
      },
      {
        id: 'nav:rules',
        group: '页面导航',
        label: '前往 分流规则',
        keywords: 'rules 分流 规则 路由 域名 直连 拦截',
        badge: app.activeNav() === 'rules' ? '当前' : undefined,
        run: () => app.setActiveNav('rules'),
      },
      {
        id: 'nav:lan',
        group: '页面导航',
        label: '前往 设备与端口',
        keywords: 'lan 局域网 设备 独立端口 代理端口 terminals ports',
        badge: app.activeNav() === 'lan' ? '当前' : undefined,
        run: () => app.setActiveNav('lan'),
      },
      {
        id: 'nav:connections',
        group: '页面导航',
        label: '前往 实时连接',
        keywords: 'connections 实时连接 连接 流量 监控 连通性',
        badge: app.activeNav() === 'connections' ? '当前' : undefined,
        run: () => app.setActiveNav('connections'),
      },
      {
        id: 'nav:settings',
        group: '页面导航',
        label: '前往 系统设置',
        keywords: 'settings 系统设置 设置 mcp 更新 版本 外观 定时刷新',
        badge: app.activeNav() === 'settings' ? '当前' : undefined,
        run: () => app.setActiveNav('settings'),
      },
      {
        id: 'act:link',
        group: '快捷操作',
        label: '粘贴分享链接添加节点',
        keywords: '导入 分享链接 vless hy2 hysteria2 trojan ss vmess tuic link node',
        badge: '导入',
        run: () => app.openNodeModal('link'),
      },
      {
        id: 'act:socks5',
        group: '快捷操作',
        label: '添加 SOCKS5 节点',
        keywords: 'socks5 手动 代理 节点 新增',
        badge: 'SOCKS5',
        run: () => app.openNodeModal('socks'),
      },
      ...(app.status().vps_supported !== false
        ? [{
            id: 'act:vps',
            group: '快捷操作',
            label: 'SSH 部署 VPS 节点',
            keywords: 'ssh vps 部署 云服务器 hysteria2',
            badge: 'VPS',
            run: () => app.openNodeModal('vps'),
          }]
        : []),
      {
        id: 'act:refresh-subs',
        group: '快捷操作',
        label: '刷新全部订阅源',
        keywords: 'refresh subs 刷新 更新 订阅 节点池',
        badge: app.subs?.() ? `${app.subs().length} 个` : undefined,
        run: () => { void app.handleRefreshSubscriptions() },
      },
      {
        id: 'act:theme',
        group: '快捷操作',
        label: '切换深色 / 浅色外观',
        keywords: 'theme dark light 深色 浅色 外观 主题',
        badge: props.theme === 'dark' ? '深色' : '浅色',
        run: () => props.onToggleTheme(),
      },
    ]

    for (const port of app.accessPorts?.() ?? []) {
      entries.push({
        id: `port:${port.id}`,
        group: '设备与端口',
        label: `独立端口 · ${port.name}`,
        keywords: `${port.name} ${port.port} ${port.outbound} 独立端口 端口`,
        badge: `:${port.port}`,
        run: () => app.setActiveNav('lan'),
      })
    }

    for (const term of app.terminals?.() ?? []) {
      const title = term.name || term.dhcp_hostname || term.ip
      entries.push({
        id: `term:${term.ip}`,
        group: '设备与端口',
        label: `局域网设备 · ${title}`,
        keywords: `${title} ${term.ip} ${term.department ?? ''} ${term.mac ?? ''} 局域网 设备`,
        badge: term.ip,
        run: () => app.setActiveNav('lan'),
      })
    }

    const delays = app.delays?.() ?? {}
    for (const nodeName of sharedExitNodes()) {
      const display = cleanNodeDisplay(nodeName)
      const measured = delays[nodeName]
      const isCurrent = currentExitNode() === nodeName
      entries.push({
        id: `exit:${nodeName}`,
        group: '切换默认出口',
        label: `切换出口 · ${display.name}`,
        keywords: `${nodeName} ${display.name} ${display.code ?? ''} 出口 切换 节点 proxy`,
        code: display.code,
        badge: measured !== undefined ? formatDelay(measured) : (isCurrent ? '当前出口' : undefined),
        activeDot: isCurrent,
        run: () => switchSharedExit(nodeName),
      })
    }

    return entries
  })

  const filteredRows = createMemo<CommandRenderRow[]>(() => {
    const needle = query().trim().toLowerCase()
    const rows: CommandRenderRow[] = []
    let lastGroup = ''

    for (const entry of allCommands()) {
      if (needle) {
        const text = `${entry.label} ${entry.keywords} ${entry.badge ?? ''}`.toLowerCase()
        if (!text.includes(needle)) continue
      }
      const showGroupTitle = entry.group !== lastGroup
      lastGroup = entry.group
      rows.push({ entry, showGroupTitle })
    }
    return rows
  })

  const executeEntry = (entry: CommandEntry) => {
    entry.run()
    props.onClose()
  }

  const isOpen = createMemo(() => Boolean(props.open))

  createEffect(
    () => isOpen(),
    (open) => {
      if (!open) return
      setQuery('')
      setActiveIndex(0)

      let active = true
      queueMicrotask(() => {
        if (active) inputEl?.focus()
      })

      const handleWindowKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          event.preventDefault()
          event.stopPropagation()
          props.onClose()
          return
        }
        if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k' && !event.defaultPrevented) {
          event.preventDefault()
          props.onClose()
        }
      }

      window.addEventListener('keydown', handleWindowKeyDown)
      return () => {
        active = false
        window.removeEventListener('keydown', handleWindowKeyDown)
      }
    },
  )

  const handleDialogKeyDown = (event: KeyboardEvent) => {
    const rows = filteredRows()
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      if (rows.length === 0) return
      const next = (activeIndex() + 1) % rows.length
      setActiveIndex(next)
      const items = listEl?.querySelectorAll<HTMLElement>('.cmd-item')
      const target = items?.[next]
      if (target && typeof target.scrollIntoView === 'function') {
        target.scrollIntoView({ block: 'nearest' })
      }
      return
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault()
      if (rows.length === 0) return
      const prev = (activeIndex() - 1 + rows.length) % rows.length
      setActiveIndex(prev)
      const items = listEl?.querySelectorAll<HTMLElement>('.cmd-item')
      const target = items?.[prev]
      if (target && typeof target.scrollIntoView === 'function') {
        target.scrollIntoView({ block: 'nearest' })
      }
      return
    }

    if (event.key === 'Enter') {
      event.preventDefault()
      const current = rows[activeIndex()]
      if (current) executeEntry(current.entry)
    }
  }

  return (
    <Show when={props.open}>
      <div class="cmd-backdrop" onClick={() => props.onClose()}>
        <div
          class="cmd-dialog"
          role="dialog"
          aria-modal="true"
          aria-label="命令面板"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={handleDialogKeyDown}
        >
          <div class="cmd-input-wrap">
            <SearchIcon size={16} />
            <input
              ref={(el) => { inputEl = el }}
              type="text"
              aria-label="搜索命令或节点"
              placeholder="搜索页面、快捷操作或出口节点…"
              value={query()}
              onInput={(event) => {
                setQuery(event.currentTarget.value)
                setActiveIndex(0)
              }}
            />
            <span class="kbd-chip">ESC</span>
          </div>

          <div ref={(el) => { listEl = el }} class="cmd-list" role="listbox" aria-label="命令结果">
            <Show
              when={filteredRows().length > 0}
              fallback={<div class="cmd-empty">未找到匹配的命令或节点</div>}
            >
              <For each={filteredRows()} keyed={(row) => row.entry.id}>
                {(row, index) => (
                  <>
                    <Show when={row().showGroupTitle}>
                      <div class="cmd-group-title" role="presentation">{row().entry.group}</div>
                    </Show>
                    <button
                      type="button"
                      role="option"
                      aria-selected={activeIndex() === index() ? 'true' : 'false'}
                      class={['cmd-item', { active: activeIndex() === index() }]}
                      onMouseEnter={() => setActiveIndex(index())}
                      onClick={() => executeEntry(row().entry)}
                    >
                      <span class="picker-option-main">
                        <Show when={row().entry.activeDot}>
                          <span class="status-dot emerald" aria-hidden="true" />
                        </Show>
                        <Show when={row().entry.code}>
                          {(code) => <span class="mono-tag">{code()}</span>}
                        </Show>
                        <span class="picker-trigger-label">{row().entry.label}</span>
                      </span>
                      <Show when={row().entry.badge}>
                        {(badge) => <span class="mono-tag">{badge()}</span>}
                      </Show>
                    </button>
                  </>
                )}
              </For>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  )
}
