import React from 'react';
import { Server, Loader2, Zap } from 'lucide-react';
import { clsx } from 'clsx';
import type { NodeInfo } from '../types';

interface NodeListProps {
  nodes: NodeInfo[];
  selectedTag?: string;
  onSelect: (tag: string) => void;
  latencies: Record<string, number | undefined>;
  testingNodes?: string[];
  onTest?: (tag: string) => void;
}

export const NodeList: React.FC<NodeListProps> = ({ nodes, selectedTag, onSelect, latencies, testingNodes = [], onTest }) => {
  const getProtocolIcon = () => {
    return <Server size={18} className="text-miao-muted" />;
  };

  return (
    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
      {nodes.map((node) => {
        const isSelected = selectedTag === node.tag;
        const latency = latencies[node.tag];
        const isTesting = testingNodes.includes(node.tag);
        
        return (
          <div
            key={node.tag}
            onClick={() => onSelect(node.tag)}
            className={clsx(
              "relative p-4 rounded-xl border cursor-pointer transition-all duration-200 group",
              isSelected 
                ? "bg-miao-green-dim border-miao-green shadow-[0_0_15px_rgba(0,171,68,0.15)]" 
                : "bg-miao-panel border-miao-border hover:border-miao-green/50 hover:bg-miao-panel/80"
            )}
          >
            <div className="flex justify-between items-start mb-3">
              <div className="p-2 rounded-lg bg-miao-bg border border-miao-border text-miao-muted group-hover:text-miao-text transition-colors">
                {getProtocolIcon()}
              </div>
              
              <div className="flex items-center gap-2">
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onTest?.(node.tag);
                  }}
                  disabled={isTesting}
                  className="relative p-1.5 rounded hover:bg-miao-border text-miao-muted hover:text-miao-green transition-colors"
                  title="Test Latency"
                >
                  <Zap size={14} className={clsx(isTesting && "opacity-0")} />
                  {isTesting && (
                    <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
                      <Loader2 size={14} className="animate-spin text-miao-green" />
                    </div>
                  )}
                </button>

                {latency !== undefined && !isTesting && (
                  <div className={clsx("px-2 py-1 rounded text-xs font-mono font-medium transition-colors", 
                    latency < 0 ? "text-miao-red bg-miao-red/10 border border-miao-red/20" :
                    latency < 100 ? "text-green-400 bg-green-400/10" :
                    latency < 300 ? "text-yellow-400 bg-yellow-400/10" : "text-orange-400 bg-orange-400/10"
                  )}>
                    {latency < 0 ? "Timeout" : `${latency}ms`}
                  </div>
                )}
              </div>
            </div>
            
            <h3 className="font-medium text-miao-text truncate mb-1" title={node.tag}>{node.tag}</h3>
            <p className="text-xs text-miao-muted truncate">{node.server}:{node.server_port}</p>
            
            {isSelected && (
              <div className="absolute inset-0 border-2 border-miao-green rounded-xl pointer-events-none" />
            )}
          </div>
        );
      })}
    </div>
  );
};
