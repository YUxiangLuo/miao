import { Shield, Trash2 } from 'lucide-react'
import { SectionCard } from './ui.jsx'
import { classNames, protocolLabel } from '../utils.js'

export function NodesCard({ nodes, onDeleteNode, onOpenAddNode }) {
  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="section-icon">
              <rect width="20" height="8" x="2" y="2" rx="2" ry="2"/>
              <rect width="20" height="8" x="2" y="14" rx="2" ry="2"/>
              <line x1="6" x2="6.01" y1="6" y2="6"/>
              <line x1="6" x2="6.01" y1="18" y2="18"/>
            </svg>
            <span>手动节点</span>
            <span className="counter-pill">{nodes.length}</span>
          </div>
        </div>
      }
    >
      <div className="list-stack">
        {nodes.length === 0 
          ? <div className="empty-block">暂无手动节点</div> 
          : nodes.map((node) => (
            <div key={node.tag} className="list-row">
              <Shield size={13} className="list-leading-icon" />
              <div className="list-row-content">
                <div className="list-row-title">{node.tag}</div>
                <div className="list-row-meta">{node.server}:{node.server_port} · {protocolLabel(node.node_type)}</div>
              </div>
              <button 
                className="icon-button subtle" 
                onClick={() => onDeleteNode(node.tag)}
              >
                <Trash2 size={13} />
              </button>
            </div>
          ))}
      </div>
    </SectionCard>
  )
}
