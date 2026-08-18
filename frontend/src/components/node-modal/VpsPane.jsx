import { useState } from 'react'
import { Rocket } from 'lucide-react'
import { Button } from '../ui.jsx'

export function VpsPane({ onDeploy, loading }) {
  const [ip, setIp] = useState('')
  const [password, setPassword] = useState('')
  const [deploying, setDeploying] = useState(false)
  const busy = loading || deploying
  const canDeploy = ip.trim().length > 0 && password.length > 0

  const handleDeploy = async () => {
    if (!canDeploy || busy) return
    setDeploying(true)
    try {
      await onDeploy({ ip: ip.trim(), password })
    } finally {
      setDeploying(false)
    }
  }

  return (
    <div className="node-pane">
      <div className="node-pane-scroll">
        <div className="form-grid single">
          <label className="field">
            <span>VPS IP 地址</span>
            <input
              value={ip}
              onChange={(event) => setIp(event.target.value)}
              placeholder="203.0.113.10"
              aria-label="VPS IP 地址"
            />
          </label>
        </div>
        <div className="form-grid single">
          <label className="field">
            <span>root 密码</span>
            <input
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder="root 登录密码"
              aria-label="root 密码"
            />
          </label>
        </div>
        <div className="vps-deploy-hint">
          密码仅用于本次部署,不会被保存。目标 VPS 需允许 root SSH 登录(端口 22),将自动安装并配置 Hysteria2 节点。
        </div>
      </div>
      <Button
        tone="primary"
        icon={<Rocket size={14} />}
        loading={busy}
        disabled={!canDeploy || busy}
        onClick={handleDeploy}
      >
        {busy ? '部署中,可能需要 1-2 分钟…' : '开始部署'}
      </Button>
    </div>
  )
}
