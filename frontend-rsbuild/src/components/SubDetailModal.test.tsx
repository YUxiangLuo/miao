import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { SubDetailModal } from './SubDetailModal'
import { subMock, subNodeMock, subNodesInfoMock } from '../testFixtures'
import type { ApiResponse, SubNodesInfo } from '../types/api'

const SUB_URL = 'https://example.com/subscription-token-abcdef'

function subNodesPayload(groups: SubNodesInfo[]): ApiResponse<SubNodesInfo[]> {
  return { success: true, message: 'ok', data: groups }
}

function stubSubNodesFetch(groups: SubNodesInfo[]) {
  rs.stubGlobal('fetch', rs.fn(async () => ({
    ok: true,
    json: async () => subNodesPayload(groups),
  })))
}

function renderModal(props = {}) {
  return render(
    <SubDetailModal
      sub={subMock({ url: SUB_URL, node_count: 2 })}
      onClose={rs.fn()}
      onToggleNode={rs.fn().mockResolvedValue(true)}
      {...props}
    />,
  )
}

describe('SubDetailModal', () => {
  afterEach(() => {
    rs.unstubAllGlobals()
  })

  it('renders nothing when sub is null', () => {
    stubSubNodesFetch([])
    const { container } = renderModal({ sub: null })
    expect(container).toBeEmptyDOMElement()
  })

  it('loads and lists the nodes of the given subscription', async () => {
    stubSubNodesFetch([
      subNodesInfoMock({ url: 'https://example.com/other', nodes: [subNodeMock({ name: '别家节点' })] }),
      subNodesInfoMock({
        url: SUB_URL,
        nodes: [
          subNodeMock({ name: '香港 01', server: 'hk.example.com', server_port: 8443 }),
          subNodeMock({ name: '日本 01', node_type: 'hysteria2', disabled: true }),
        ],
      }),
    ])
    renderModal()

    expect(await screen.findByText('香港 01')).toBeInTheDocument()
    expect(screen.getByText('日本 01')).toBeInTheDocument()
    expect(screen.queryByText('别家节点')).not.toBeInTheDocument()
    expect(screen.getByText('hk.example.com:8443')).toBeInTheDocument()
    // 头部统计：共 2 个 · 禁用 1
    expect(screen.getByText('共 2 个节点 · 禁用 1')).toBeInTheDocument()
    // 禁用节点排在最前面（日本 01 被禁用，尽管原始顺序在香港 01 之后）
    const titles = [...document.querySelectorAll('.sub-node-row .list-row-title')].map((el) => el.textContent)
    expect(titles).toEqual(['日本 01', '香港 01'])
  })

  it('toggles a node via onToggleNode and reloads the list', async () => {
    const user = userEvent.setup()
    let disabled = false
    rs.stubGlobal('fetch', rs.fn(async () => ({
      ok: true,
      json: async () => subNodesPayload([
        subNodesInfoMock({ url: SUB_URL, nodes: [subNodeMock({ name: '香港 01', disabled })] }),
      ]),
    })))
    const onToggleNode = rs.fn(async (_sub: string, _name: string, next: boolean) => {
      disabled = next
      return true
    })
    renderModal({ onToggleNode })

    const toggle = await screen.findByRole('switch', { name: '禁用节点 香港 01' })
    expect(toggle).toHaveAttribute('aria-checked', 'true')
    await user.click(toggle)

    expect(onToggleNode).toHaveBeenCalledWith(SUB_URL, '香港 01', true)
    // 切换成功后重新拉取，开关翻转为「已禁用」
    await waitFor(() => {
      expect(screen.getByRole('switch', { name: '启用节点 香港 01' })).toHaveAttribute('aria-checked', 'false')
    })
  })

  it('shows an error block when loading fails', async () => {
    rs.stubGlobal('fetch', rs.fn(async () => { throw new Error('network down') }))
    renderModal()

    expect(await screen.findByText('加载节点列表失败')).toBeInTheDocument()
  })

  it('shows an empty block when the subscription has no fetched nodes', async () => {
    stubSubNodesFetch([subNodesInfoMock({ url: SUB_URL })])
    renderModal()

    expect(await screen.findByText(/暂无节点/)).toBeInTheDocument()
  })

  it('ignores an older response after switching subscriptions', async () => {
    const resolvers: Array<(response: Response) => void> = []
    rs.stubGlobal('fetch', rs.fn(() => new Promise<Response>((resolve) => {
      resolvers.push(resolve)
    })))
    const firstSub = subMock({ url: 'https://example.com/first' })
    const secondSub = subMock({ url: 'https://example.com/second' })
    const { rerender } = renderModal({ sub: firstSub })
    await waitFor(() => expect(resolvers).toHaveLength(1))

    rerender(
      <SubDetailModal
        sub={secondSub}
        onClose={rs.fn()}
        onToggleNode={rs.fn().mockResolvedValue(true)}
      />,
    )
    await waitFor(() => expect(resolvers).toHaveLength(2))

    await act(async () => {
      resolvers[1]({
        ok: true,
        json: async () => subNodesPayload([
          subNodesInfoMock({ url: secondSub.url, nodes: [subNodeMock({ name: '新订阅节点' })] }),
        ]),
      } as Response)
    })
    expect(screen.getByText('新订阅节点')).toBeInTheDocument()

    await act(async () => {
      resolvers[0]({
        ok: true,
        json: async () => subNodesPayload([
          subNodesInfoMock({ url: firstSub.url, nodes: [subNodeMock({ name: '旧订阅节点' })] }),
        ]),
      } as Response)
    })
    expect(screen.queryByText('旧订阅节点')).not.toBeInTheDocument()
    expect(screen.getByText('新订阅节点')).toBeInTheDocument()
  })

  it('ignores a reload from a mutation on the previously open subscription', async () => {
    const user = userEvent.setup()
    const firstSub = subMock({ url: 'https://example.com/first' })
    const secondSub = subMock({ url: 'https://example.com/second' })
    const fetchMock = rs.fn(async () => ({
      ok: true,
      json: async () => subNodesPayload([
        subNodesInfoMock({ url: firstSub.url, nodes: [subNodeMock({ name: '旧订阅节点' })] }),
        subNodesInfoMock({ url: secondSub.url, nodes: [subNodeMock({ name: '新订阅节点' })] }),
      ]),
    }))
    rs.stubGlobal('fetch', fetchMock)
    let resolveToggle: (value: boolean) => void = () => {}
    const onToggleNode = rs.fn(() => new Promise<boolean>((resolve) => {
      resolveToggle = resolve
    }))
    const { rerender } = renderModal({ sub: firstSub, onToggleNode })

    await user.click(await screen.findByRole('switch', { name: '禁用节点 旧订阅节点' }))
    rerender(
      <SubDetailModal
        sub={secondSub}
        onClose={rs.fn()}
        onToggleNode={onToggleNode}
      />,
    )
    expect(await screen.findByText('新订阅节点')).toBeInTheDocument()

    await act(async () => resolveToggle(true))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(screen.queryByText('旧订阅节点')).not.toBeInTheDocument()
    expect(screen.getByText('新订阅节点')).toBeInTheDocument()
  })

  it('lists stale disabled entries and clears them via the enable path', async () => {
    const user = userEvent.setup()
    let stale = ['剩余流量：45.17 GB']
    rs.stubGlobal('fetch', rs.fn(async () => ({
      ok: true,
      json: async () => subNodesPayload([
        subNodesInfoMock({
          url: SUB_URL,
          nodes: [subNodeMock({ name: '香港 01', disabled: true })],
          stale_disabled: stale,
        }),
      ]),
    })))
    const onToggleNode = rs.fn(async () => {
      stale = []
      return true
    })
    renderModal({ onToggleNode })

    // 失配条目单独成块展示，不计入生效禁用数
    expect(await screen.findByText('剩余流量：45.17 GB')).toBeInTheDocument()
    expect(screen.getByText('共 1 个节点 · 禁用 1')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '移除失效禁用 剩余流量：45.17 GB' }))

    // 走 disabled=false 的清理路径
    expect(onToggleNode).toHaveBeenCalledWith(SUB_URL, '剩余流量：45.17 GB', false)
    await waitFor(() => {
      expect(screen.queryByText('剩余流量：45.17 GB')).not.toBeInTheDocument()
    })
  })
})
