import { 
  ArrowUp, 
  ArrowDown, 
  Power,
  LoaderCircle 
} from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { classNames, formatUptime, formatSpeed } from '../utils.js'

export function StatusCard({ status, traffic, loadingAction, onToggleService }) {
  return (
    <SectionCard className="status-card" bodyClassName="status-card-body" header={null}>
      <div className="status-left-wrap">
        <div className="status-pill-icon"><span className="status-pill-dot" /></div>
        <div className="status-copy">
          <div className="status-title">
            Sing-box {status.initializing ? '初始化中' : status.running ? '运行中' : '已停止'}
          </div>
          <div className="status-subtitle">
            {status.running 
              ? `PID: ${status.pid ?? '--'} · 运行时长: ${formatUptime(status.uptime_secs)}` 
              : status.initializing 
                ? '正在获取订阅并启动服务…' 
                : '等待启动服务'}
          </div>
        </div>
      </div>

      <div className="traffic-chip">
        <div className="traffic-item">
          <ArrowUp size={14} className="traffic-icon up" />
          <span>{formatSpeed(traffic.up)}</span>
        </div>
        <div className="traffic-item">
          <ArrowDown size={14} className="traffic-icon down" />
          <span>{formatSpeed(traffic.down)}</span>
        </div>
      </div>

      <div className="status-card-spacer" />
      <Button 
        tone={status.running ? 'danger' : 'success'} 
        icon={<Power size={14} />} 
        loading={loadingAction === 'start' || loadingAction === 'stop' || status.initializing} 
        disabled={loadingAction === 'start' || loadingAction === 'stop' || status.initializing} 
        onClick={onToggleService}
      >
        {status.running ? '停止服务' : '启动服务'}
      </Button>
    </SectionCard>
  )
}
