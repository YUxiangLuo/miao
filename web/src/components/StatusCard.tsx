import React from 'react';
import { Activity, Power, Clock } from 'lucide-react';
import { clsx } from 'clsx';
import type { StatusData } from '../types';

interface StatusCardProps {
  status: StatusData | null;
  loading: boolean;
  onToggle: () => void;
  toggling: boolean;
}

export const StatusCard: React.FC<StatusCardProps> = ({ status, loading, onToggle, toggling }) => {
  const isRunning = status?.running;

  const formatUptime = (secs?: number) => {
    if (!secs) return '0s';
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = secs % 60;
    return `${h}h ${m}m ${s}s`;
  };

  return (
    <div className="bg-miao-panel border border-miao-border rounded-xl p-6 flex flex-col justify-between relative overflow-hidden group">
      <div className="absolute top-0 right-0 p-4 opacity-10 group-hover:opacity-20 transition-opacity">
        <Activity size={64} className={isRunning ? 'text-miao-green' : 'text-miao-muted'} />
      </div>

      <div>
        <h2 className="text-miao-muted text-sm font-medium uppercase tracking-wider mb-1">Service Status</h2>
        <div className="flex items-center gap-3">
          <div className={clsx("w-3 h-3 rounded-full animate-pulse", isRunning ? "bg-miao-green" : "bg-miao-red")} />
          <span className={clsx("text-2xl font-bold", isRunning ? "text-miao-text" : "text-miao-muted")}>
            {loading ? "Checking..." : isRunning ? "Active" : "Stopped"}
          </span>
        </div>
      </div>

      <div className="mt-6 space-y-2">
        {isRunning && (
          <>
            <div className="flex items-center justify-between text-sm">
              <span className="flex items-center gap-2 text-miao-muted"><Clock size={14} /> Uptime</span>
              <span className="font-mono text-miao-green">{formatUptime(status?.uptime_secs)}</span>
            </div>
            <div className="flex items-center justify-between text-sm">
              <span className="flex items-center gap-2 text-miao-muted"><Activity size={14} /> PID</span>
              <span className="font-mono">{status?.pid}</span>
            </div>
          </>
        )}
      </div>

      <button
        onClick={onToggle}
        disabled={toggling}
        className={clsx(
          "mt-6 w-full py-2.5 rounded-lg font-medium flex items-center justify-center gap-2 transition-all",
          isRunning 
            ? "bg-miao-bg border border-miao-red text-miao-red hover:bg-miao-red/10" 
            : "bg-miao-green text-white hover:bg-miao-green-hover shadow-lg shadow-miao-green/20",
          toggling && "opacity-50 cursor-not-allowed"
        )}
      >
        <Power size={18} />
        {toggling ? "Processing..." : isRunning ? "Stop Service" : "Start Service"}
      </button>
    </div>
  );
};
