import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { NodesCard } from './NodesCard'
import { nodeMock } from '../testFixtures'

const nodes = [
  nodeMock({ tag: '香港节点', node_type: 'hysteria2' }),
  nodeMock({ tag: 'vps-1', node_type: 'trojan' }),
]

function renderCard(props = {}) {
  return render(
    <NodesCard
      nodes={nodes}
      isInitializing={false}
      onDeleteNode={rs.fn()}
      onOpenAddNode={rs.fn()}
      {...props}
    />,
  )
}

describe('NodesCard', () => {
  it('keeps the protocol badge as a direct row child so the grid can right-align it', () => {
    renderCard()

    const rows = document.querySelectorAll('.list-row.node-row')
    expect(rows).toHaveLength(nodes.length)
    for (const row of rows) {
      // 徽章必须是行的直接子元素：.node-row 的三列网格按直接子元素分列右对齐，
      // 塞回 .list-row-title 里徽章会退回紧跟文字左侧，各行右缘不再对齐。
      expect(row.querySelector(':scope > .badge')).not.toBeNull()
      expect(row.querySelector('.list-row-title .badge')).toBeNull()
      expect(row.querySelector(':scope > .icon-button')).not.toBeNull()
    }
    expect(screen.getByText('hysteria2')).toBeInTheDocument()
    expect(screen.getByText('trojan')).toBeInTheDocument()
  })

  it('truncates long tags inside the content column without squeezing the badge', () => {
    renderCard({ nodes: [nodeMock({ tag: 'very-long-node-tag-'.repeat(8) })] })

    const row = document.querySelector('.list-row.node-row')!
    expect(row.querySelector('.list-row-content > .list-row-title')).not.toBeNull()
    expect(row.querySelector(':scope > .badge')).not.toBeNull()
  })

  it('calls onDeleteNode with the tag when the delete button is clicked', async () => {
    const user = userEvent.setup()
    const onDeleteNode = rs.fn()
    renderCard({ onDeleteNode })

    await user.click(screen.getByRole('button', { name: '删除节点 香港节点' }))
    expect(onDeleteNode).toHaveBeenCalledWith('香港节点')
  })

  it('renders an empty state when there are no manual nodes', () => {
    renderCard({ nodes: [] })

    expect(screen.getByText('暂无手动节点')).toBeInTheDocument()
  })
})
