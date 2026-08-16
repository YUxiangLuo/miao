import { memo } from 'react'
import { ListTree, Plus, Server, Trash2 } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { protocolLabel } from '../utils.js'

const NodeRow = memo(function NodeRow({ node, onDelete, disabled }) {
  return (
    <div className="list-row">
      <Server size={13} className="list-leading-icon" />
      <div className="list-row-content">
        <div className="list-row-title">{node.tag}</div>
        <div className="list-row-meta" title={`${node.server}:${node.server_port}`}>
          {node.server}:{node.server_port} · {protocolLabel(node.node_type)}
        </div>
      </div>
      <button
        className="icon-button subtle"
        onClick={() => onDelete(node.tag)}
        disabled={disabled}
        aria-label={`删除节点 ${node.tag}`}
      >
        <Trash2 size={13} />
      </button>
    </div>
  )
})

export function NodesCard({ nodes, isInitializing, onDeleteNode, onOpenAddNode }) {
  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <ListTree size={14} className="section-icon" />
            <span>手动节点</span>
            <span className="counter-pill">{nodes.length}</span>
          </div>
          <Button
            tone="secondary"
            size="sm"
            icon={<Plus size={12} />}
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
