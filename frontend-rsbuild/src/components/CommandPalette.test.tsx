import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { CommandPalette, useCommandShortcut } from './CommandPalette'
import type { useAppController } from '../hooks/useAppController'

const FLAG_US = '\u{1F1FA}\u{1F1F8}'
const FLAG_JP = '\u{1F1EF}\u{1F1F5}'

type AppController = ReturnType<typeof useAppController>

function createMockApp(overrides: Partial<Record<string, unknown>> = {}): AppController {
  return {
    activeNav: () => 'studio',
    setActiveNav: rs.fn(),
    openNodeModal: rs.fn(),
    handleRefreshSubscriptions: rs.fn().mockResolvedValue(undefined),
    handleSwitchProxy: rs.fn().mockResolvedValue(undefined),
    handleSetNodeSelect: rs.fn().mockResolvedValue(undefined),
    primaryGroupName: () => 'proxy',
    primaryGroup: () => ({
      type: 'Selector',
      now: `${FLAG_JP} 东京高速 01`,
      all: [`${FLAG_JP} 东京高速 01`, `${FLAG_US} 洛杉矶原生 02`],
    }),
    delays: () => ({ [`${FLAG_JP} 东京高速 01`]: 36, [`${FLAG_US} 洛杉矶原生 02`]: 128 }),
    subs: () => [{ url: 'https://sub.example.com', success: true, node_count: 12, disabled_count: 0, state: 'ready' }],
    terminals: () => [{ ip: '192.168.8.21', name: '运营工位 A', department: 'TikTok组' }],
    accessPorts: () => [{ id: 'p1', name: 'TikTok 英区店', port: 20101, outbound: 'uk-line', has_auth: false, blocked: false }],
    status: () => ({
      ready: true,
      running: true,
      initializing: false,
      node_select: 'manual',
      requested_node_select: 'manual',
    }),
    ...overrides,
  } as unknown as AppController
}

describe('useCommandShortcut', () => {
  it('invokes toggle callback on Ctrl+K and Meta+K', () => {
    const onToggle = rs.fn()
    function Harness() {
      useCommandShortcut(onToggle)
      return <div>shortcut-ready</div>
    }
    render(() => <Harness />)

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true })
    expect(onToggle).toHaveBeenCalledTimes(1)

    fireEvent.keyDown(window, { key: 'K', metaKey: true })
    expect(onToggle).toHaveBeenCalledTimes(2)
  })
})

describe('CommandPalette', () => {
  it('navigates across the 4 studio workspaces and closes on execution', async () => {
    const user = userEvent.setup()
    const app = createMockApp()
    const onClose = rs.fn()

    render(() => (
      <CommandPalette
        open
        onClose={onClose}
        app={app}
        onToggleTheme={rs.fn()}
        theme="light"
      />
    ))

    expect(screen.getByRole('dialog', { name: '全局命令面板' })).toBeInTheDocument()
    await user.click(screen.getByRole('option', { name: /前往 全球线路/ }))
    expect(app.setActiveNav).toHaveBeenCalledWith('routes')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('filters commands by keyword and executes quick actions via keyboard Enter', async () => {
    const user = userEvent.setup()
    const app = createMockApp()
    const onClose = rs.fn()
    const onToggleTheme = rs.fn()

    render(() => (
      <CommandPalette
        open
        onClose={onClose}
        app={app}
        onToggleTheme={onToggleTheme}
        theme="light"
      />
    ))

    const input = screen.getByRole('textbox', { name: '搜索命令或节点' })
    await user.type(input, 'SOCKS5')
    const options = screen.getAllByRole('option')
    expect(options).toHaveLength(1)
    expect(options[0]).toHaveTextContent('录入独享静态住宅 SOCKS5')

    await user.keyboard('{Enter}')
    expect(app.openNodeModal).toHaveBeenCalledWith('socks-residential')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('switches shared exit nodes with ArrowDown + Enter and supports Escape / backdrop dismissal', async () => {
    const user = userEvent.setup()
    const app = createMockApp()
    const [open, setOpen] = createSignal(true)

    const { container } = render(() => (
      <CommandPalette
        open={open()}
        onClose={() => setOpen(false)}
        app={app}
        onToggleTheme={rs.fn()}
        theme="dark"
      />
    ))

    const input = screen.getByRole('textbox', { name: '搜索命令或节点' })
    await user.type(input, '出口')
    const exitOptions = screen.getAllByRole('option')
    expect(exitOptions).toHaveLength(2)
    expect(exitOptions[0]).toHaveTextContent('JP')
    expect(exitOptions[0]).toHaveTextContent('东京高速 01')
    expect(exitOptions[1]).toHaveTextContent('US')
    expect(exitOptions[1]).toHaveTextContent('洛杉矶原生 02')

    await user.keyboard('{ArrowDown}{Enter}')
    expect(app.handleSwitchProxy).toHaveBeenCalledWith('proxy', `${FLAG_US} 洛杉矶原生 02`)
    expect(open()).toBe(false)

    setOpen(true)
    flush()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    await user.keyboard('{Escape}')
    expect(open()).toBe(false)

    setOpen(true)
    flush()
    const backdrop = container.querySelector('.cmd-backdrop')!
    await user.click(backdrop)
    expect(open()).toBe(false)
  })

  it('switches the VPS quick action to link import when vps_supported is false', async () => {
    const user = userEvent.setup()
    const app = createMockApp({
      status: () => ({
        ready: true,
        running: true,
        initializing: false,
        node_select: 'manual',
        requested_node_select: 'manual',
        vps_supported: false,
        platform: 'windows',
      }),
    })
    const onClose = rs.fn()

    render(() => (
      <CommandPalette
        open
        onClose={onClose}
        app={app}
        onToggleTheme={rs.fn()}
        theme="light"
      />
    ))

    expect(screen.queryByRole('option', { name: /SSH 部署海外 VPS/ })).toBeNull()
    await user.click(screen.getByRole('option', { name: /导入独享专线 \/ 分享链接/ }))
    expect(app.openNodeModal).toHaveBeenCalledWith('link')
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
