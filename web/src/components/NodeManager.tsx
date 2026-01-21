import React, { useState } from 'react';
import { Plus, Trash2, Server, Save, X } from 'lucide-react';
import type { NodeInfo } from '../types';
import { api } from '../lib/api';
import { clsx } from 'clsx';

interface NodeManagerProps {
  nodes: NodeInfo[];
  onUpdate: () => void;
}

export const NodeManager: React.FC<NodeManagerProps> = ({ nodes, onUpdate }) => {
  const manualNodes = nodes.filter(n => n.source === 'manual');
  const [isAdding, setIsAdding] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  // Form State
  const [formData, setFormData] = useState({
    node_type: 'hysteria2',
    tag: '',
    server: '',
    server_port: 443,
    password: '',
    sni: '',
    cipher: '2022-blake3-aes-128-gcm' // for ss
  });

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    const { name, value } = e.target;
    setFormData(prev => ({
      ...prev,
      [name]: name === 'server_port' ? parseInt(value) || 0 : value
    }));
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!formData.tag || !formData.server || !formData.password) return;

    setSubmitting(true);
    const res = await api.addNode(formData);
    setSubmitting(false);

    if (res.success) {
      setIsAdding(false);
      setFormData({
        node_type: 'hysteria2',
        tag: '',
        server: '',
        server_port: 443,
        password: '',
        sni: '',
        cipher: '2022-blake3-aes-128-gcm'
      });
      onUpdate();
    } else {
      alert(`Failed to add node: ${res.message}`);
    }
  };

  const handleDelete = async (tag: string) => {
    if (!confirm(`Delete manual node "${tag}"?`)) return;
    await api.deleteNode(tag);
    onUpdate();
  };

  return (
    <div className="bg-miao-panel border border-miao-border rounded-xl overflow-hidden flex flex-col h-full">
      <div className="p-4 border-b border-miao-border flex items-center justify-between bg-miao-bg/30">
        <h2 className="text-miao-text font-medium flex items-center gap-2">
          <Server size={18} className="text-miao-green" />
          Manual Nodes
        </h2>
        <button
          onClick={() => setIsAdding(!isAdding)}
          className={clsx(
            "p-2 rounded-lg transition-colors flex items-center gap-2 text-sm font-medium",
            isAdding ? "bg-miao-red/10 text-miao-red hover:bg-miao-red/20" : "bg-miao-green text-white hover:bg-miao-green-hover"
          )}
        >
          {isAdding ? <><X size={16} /> Cancel</> : <><Plus size={16} /> Add Node</>}
        </button>
      </div>

      <div className="flex-1 overflow-auto p-4 space-y-4">
        {isAdding && (
          <form onSubmit={handleSubmit} className="bg-miao-bg border border-miao-border rounded-xl p-4 space-y-4 mb-6 shadow-lg shadow-black/20">
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <label className="block text-xs text-miao-muted uppercase font-bold mb-1">Protocol</label>
                <select 
                  name="node_type" 
                  value={formData.node_type} 
                  onChange={handleInputChange}
                  className="w-full bg-miao-panel border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text focus:border-miao-green outline-none"
                >
                  <option value="hysteria2">Hysteria2</option>
                  <option value="anytls">AnyTLS</option>
                  <option value="ss">Shadowsocks</option>
                </select>
              </div>
              <div>
                <label className="block text-xs text-miao-muted uppercase font-bold mb-1">Tag (Name)</label>
                <input 
                  type="text" 
                  name="tag"
                  value={formData.tag}
                  onChange={handleInputChange}
                  placeholder="My Node"
                  className="w-full bg-miao-panel border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text focus:border-miao-green outline-none"
                  required
                />
              </div>
              
              <div>
                <label className="block text-xs text-miao-muted uppercase font-bold mb-1">Server Address</label>
                <input 
                  type="text" 
                  name="server"
                  value={formData.server}
                  onChange={handleInputChange}
                  placeholder="example.com"
                  className="w-full bg-miao-panel border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text focus:border-miao-green outline-none"
                  required
                />
              </div>
              <div>
                <label className="block text-xs text-miao-muted uppercase font-bold mb-1">Port</label>
                <input 
                  type="number" 
                  name="server_port"
                  value={formData.server_port}
                  onChange={handleInputChange}
                  className="w-full bg-miao-panel border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text focus:border-miao-green outline-none"
                  required
                />
              </div>

              <div className="md:col-span-2">
                <label className="block text-xs text-miao-muted uppercase font-bold mb-1">Password</label>
                <input 
                  type="text" 
                  name="password"
                  value={formData.password}
                  onChange={handleInputChange}
                  className="w-full bg-miao-panel border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text focus:border-miao-green outline-none"
                  required
                />
              </div>

              {formData.node_type === 'ss' ? (
                <div className="md:col-span-2">
                  <label className="block text-xs text-miao-muted uppercase font-bold mb-1">Cipher</label>
                  <select 
                    name="cipher" 
                    value={formData.cipher} 
                    onChange={handleInputChange}
                    className="w-full bg-miao-panel border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text focus:border-miao-green outline-none"
                  >
                    <option value="2022-blake3-aes-128-gcm">2022-blake3-aes-128-gcm</option>
                    <option value="2022-blake3-aes-256-gcm">2022-blake3-aes-256-gcm</option>
                    <option value="aes-128-gcm">aes-128-gcm</option>
                    <option value="aes-256-gcm">aes-256-gcm</option>
                    <option value="chacha20-poly1305">chacha20-poly1305</option>
                  </select>
                </div>
              ) : (
                <div className="md:col-span-2">
                  <label className="block text-xs text-miao-muted uppercase font-bold mb-1">SNI (Optional)</label>
                  <input 
                    type="text" 
                    name="sni"
                    value={formData.sni}
                    onChange={handleInputChange}
                    placeholder="Same as server if empty"
                    className="w-full bg-miao-panel border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text focus:border-miao-green outline-none"
                  />
                </div>
              )}
            </div>

            <button 
              type="submit" 
              disabled={submitting}
              className="w-full bg-miao-green hover:bg-miao-green-hover text-white font-medium py-2 rounded-lg transition-colors flex items-center justify-center gap-2 disabled:opacity-50"
            >
              {submitting ? <Server className="animate-spin" size={16} /> : <Save size={16} />}
              Save Node
            </button>
          </form>
        )}

        {manualNodes.length === 0 ? (
          <div className="text-center text-miao-muted py-8 text-sm">
            No manual nodes configured.
          </div>
        ) : (
          <div className="space-y-2">
            {manualNodes.map(node => (
              <div key={node.tag} className="flex items-center justify-between p-3 bg-miao-bg rounded-lg border border-miao-border group hover:border-miao-green/30 transition-colors">
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="font-bold text-sm text-miao-text">{node.tag}</span>
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-miao-panel border border-miao-border text-miao-muted uppercase">
                      {node.protocol || 'Unknown'}
                    </span>
                  </div>
                  <div className="text-xs text-miao-muted font-mono">
                    {node.server}:{node.server_port}
                  </div>
                </div>
                <button 
                  onClick={() => handleDelete(node.tag)}
                  className="p-2 text-miao-muted hover:text-miao-red hover:bg-miao-red/10 rounded-lg transition-colors opacity-0 group-hover:opacity-100"
                >
                  <Trash2 size={16} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
