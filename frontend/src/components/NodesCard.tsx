import { memo } from 'react'
import { Plus, Server, Trash2 } from 'lucide-react'
import { ICON } from '../tokens'
import { Button, SectionCard } from './ui'
import { protocolTone, classNames } from '../utils'
import type { NodeInfo } from '../types/api'

interface NodeRowProps {
  node: NodeInfo
  onDelete: (tag: string) => void
  disabled: boolean
}

const NodeRow = memo(function NodeRow({ node, onDelete, disabled }: NodeRowProps) {
  return (
    <div className="list-row node-row">
      <div className="list-row-content">
        <div className="list-row-title structured">
          <span className="rule-value" title={node.tag}>{node.tag}</span>
          <span className={classNames('badge', protocolTone(node.node_type))}>{node.node_type}</span>
        </div>
      </div>
      <button
        className="icon-button subtle"
        onClick={() => onDelete(node.tag)}
        disabled={disabled}
        aria-label={`删除节点 ${node.tag}`}
      >
        <Trash2 size={ICON.xs} />
      </button>
    </div>
  )
})

export interface NodesCardProps {
  nodes: NodeInfo[]
  isInitializing: boolean
  onDeleteNode: (tag: string) => void
  onOpenAddNode: () => void
}

export function NodesCard({ nodes, isInitializing, onDeleteNode, onOpenAddNode }: NodesCardProps) {
  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <Server size={ICON.sm} className="section-icon" />
            <span>手动节点</span>
            <span className={classNames('badge', 'counter-pill')}>{nodes.length}</span>
          </div>
          <Button
            tone="secondary"
            size="sm"
            icon={<Plus size={ICON.xs} />}
            disabled={isInitializing}
            onClick={onOpenAddNode}
          >
            添加
          </Button>
        </div>
      }
    >
      <div className="list-stack">
        {nodes.length === 0 
          ? <div className="empty-block">暂无手动节点</div> 
          : nodes.map((node) => (
            <NodeRow
              key={node.tag}
              node={node}
              onDelete={onDeleteNode}
              disabled={isInitializing}
            />
          ))}
      </div>
    </SectionCard>
  )
}
