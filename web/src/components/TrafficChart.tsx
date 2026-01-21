import React, { useEffect, useState } from 'react';
import { AreaChart, Area, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts';
import { ArrowDown, ArrowUp } from 'lucide-react';

interface TrafficData {
  time: string;
  up: number;
  down: number;
}

export const TrafficChart: React.FC = () => {
  const [data, setData] = useState<TrafficData[]>(Array(20).fill({ time: '', up: 0, down: 0 }));
  const [currentSpeed, setCurrentSpeed] = useState({ up: 0, down: 0 });

  useEffect(() => {
    // Connect to sing-box/Clash API for traffic
    const ws = new WebSocket(`ws://${window.location.hostname}:6262/traffic?token=`);
    
    ws.onmessage = (event) => {
      try {
        const stats = JSON.parse(event.data);
        const now = new Date();
        const timeStr = `${now.getHours()}:${now.getMinutes()}:${now.getSeconds()}`;
        
        // stats.up and stats.down are in bytes per second
        const up = stats.up || 0;
        const down = stats.down || 0;

        setCurrentSpeed({ up, down });

        setData(prev => {
          const newData = [...prev, { time: timeStr, up, down }];
          return newData.slice(-20);
        });
      } catch (e) {
        // console.error(e);
      }
    };

    return () => {
      ws.close();
    };
  }, []);

  const formatSpeed = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B/s`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB/s`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB/s`;
  };

  return (
    <div className="bg-miao-panel border border-miao-border rounded-xl p-6 flex flex-col h-full">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-miao-muted text-sm font-medium uppercase tracking-wider">Traffic Monitor</h2>
        <div className="flex gap-4">
          <div className="flex items-center gap-1.5 text-miao-green">
            <ArrowDown size={16} />
            <span className="font-mono font-bold">{formatSpeed(currentSpeed.down)}</span>
          </div>
          <div className="flex items-center gap-1.5 text-blue-400">
            <ArrowUp size={16} />
            <span className="font-mono font-bold">{formatSpeed(currentSpeed.up)}</span>
          </div>
        </div>
      </div>
      
      <div className="flex-1 min-h-[150px]">
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={data}>
            <defs>
              <linearGradient id="colorDown" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#00AB44" stopOpacity={0.3}/>
                <stop offset="95%" stopColor="#00AB44" stopOpacity={0}/>
              </linearGradient>
              <linearGradient id="colorUp" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#60A5FA" stopOpacity={0.3}/>
                <stop offset="95%" stopColor="#60A5FA" stopOpacity={0}/>
              </linearGradient>
            </defs>
            <XAxis dataKey="time" hide />
            <YAxis hide domain={[0, 'auto']} />
            <Tooltip 
              contentStyle={{ backgroundColor: '#1E1E1E', borderColor: '#333' }}
              itemStyle={{ color: '#E0E0E0' }}
              labelStyle={{ display: 'none' }}
              formatter={(value: number | undefined) => formatSpeed(value || 0)}
            />
            <Area 
              type="monotone" 
              dataKey="down" 
              stroke="#00AB44" 
              strokeWidth={2}
              fillOpacity={1} 
              fill="url(#colorDown)" 
              isAnimationActive={false}
            />
            <Area 
              type="monotone" 
              dataKey="up" 
              stroke="#60A5FA" 
              strokeWidth={2}
              fillOpacity={1} 
              fill="url(#colorUp)" 
              isAnimationActive={false}
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
};
