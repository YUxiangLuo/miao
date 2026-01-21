import React, { useState } from 'react';
import { RefreshCw, Trash2, Plus, Link, Check, AlertCircle } from 'lucide-react';
import type { SubStatus } from '../types';
import { clsx } from 'clsx';
import { api } from '../lib/api';

interface SubManagerProps {
  subs: SubStatus[];
  onUpdate: () => void;
}

export const SubManager: React.FC<SubManagerProps> = ({ subs, onUpdate }) => {
  const [newUrl, setNewUrl] = useState('');
  const [adding, setAdding] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [deletingUrl, setDeletingUrl] = useState<string | null>(null);

  const handleAdd = async () => {
    if (!newUrl) return;
    setAdding(true);
    try {
      const res = await api.addSub(newUrl);
      if (res.success) {
        setNewUrl('');
        onUpdate();
      } else {
        alert(`Failed to add subscription: ${res.message}`);
      }
    } catch (e) {
      alert('Failed to add subscription. Check logs.');
    } finally {
      setAdding(false);
    }
  };

  const handleDelete = async (url: string) => {
    if (!confirm('Remove this subscription?')) return;
    setDeletingUrl(url);
    try {
      const res = await api.deleteSub(url);
      if (res.success) {
        onUpdate();
      } else {
        alert(`Failed to delete subscription: ${res.message}`);
      }
    } catch (e) {
      alert('Failed to delete subscription. Check logs.');
    } finally {
      setDeletingUrl(null);
    }
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      const res = await api.refreshSubs();
      if (res.success) {
        onUpdate();
      } else {
        alert(`Failed to refresh subscriptions: ${res.message}`);
      }
    } catch (e) {
      alert('Failed to refresh subscriptions. Check logs.');
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div className="bg-miao-panel border border-miao-border rounded-xl overflow-hidden">
      <div className="p-4 border-b border-miao-border flex items-center justify-between">
        <h2 className="text-miao-text font-medium flex items-center gap-2">
          <Link size={18} className="text-miao-green" />
          Subscriptions
        </h2>
        <button 
          onClick={handleRefresh}
          disabled={refreshing}
          className="p-2 hover:bg-miao-bg rounded-lg text-miao-muted hover:text-miao-green transition-colors disabled:opacity-50"
          title="Refresh All"
        >
          <RefreshCw size={18} className={clsx(refreshing && "animate-spin")} />
        </button>
      </div>

      <div className="divide-y divide-miao-border">
        {subs.map((sub) => (
          <div key={sub.url} className="p-4 flex items-center justify-between group hover:bg-miao-bg/50 transition-colors">
            <div className="flex-1 min-w-0 mr-4">
              <div className="flex items-center gap-2 mb-1">
                <div className="font-mono text-xs text-miao-muted truncate w-full" title={sub.url}>
                  {sub.url}
                </div>
              </div>
              <div className="flex items-center gap-3 text-xs">
                 {sub.success ? (
                   <span className="flex items-center gap-1 text-green-400">
                     <Check size={12} /> {sub.node_count} nodes
                   </span>
                 ) : (
                   <span className="flex items-center gap-1 text-red-400" title={sub.error}>
                     <AlertCircle size={12} /> Error
                   </span>
                 )}
              </div>
            </div>
            <button 
              onClick={() => handleDelete(sub.url)}
              disabled={deletingUrl === sub.url}
              className={clsx(
                "p-2 rounded-lg transition-all",
                deletingUrl === sub.url 
                  ? "text-miao-green bg-miao-green/10 opacity-100" 
                  : "text-miao-muted hover:text-red-400 hover:bg-miao-red/10 opacity-0 group-hover:opacity-100"
              )}
            >
              {deletingUrl === sub.url ? <RefreshCw size={16} className="animate-spin" /> : <Trash2 size={16} />}
            </button>
          </div>
        ))}

        {subs.length === 0 && (
          <div className="p-8 text-center text-miao-muted text-sm">
            No subscriptions added yet.
          </div>
        )}

        <div className="p-4 bg-miao-bg/30">
          <div className="flex gap-2">
            <input
              type="text"
              value={newUrl}
              onChange={(e) => setNewUrl(e.target.value)}
              placeholder="Paste subscription URL (Clash/Meta format)..."
              className="flex-1 bg-miao-bg border border-miao-border rounded-lg px-3 py-2 text-sm text-miao-text placeholder-miao-muted focus:outline-none focus:border-miao-green transition-colors"
              onKeyDown={(e) => e.key === 'Enter' && handleAdd()}
            />
            <button
              onClick={handleAdd}
              disabled={adding || !newUrl}
              className="bg-miao-green hover:bg-miao-green-hover disabled:opacity-50 disabled:cursor-not-allowed text-white px-4 py-2 rounded-lg font-medium text-sm transition-colors flex items-center gap-2"
            >
              {adding ? <RefreshCw size={16} className="animate-spin" /> : <Plus size={16} />}
              {adding ? "Adding..." : "Add"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
