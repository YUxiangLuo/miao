import { useState } from 'react'
import { Rocket, X } from 'lucide-react'
import { ICON } from '../../tokens'
import { Button } from '../ui'
import type { VpsDeployRequest } from '../../types/api'

export interface VpsPaneProps {
  onDeploy: (req: VpsDeployRequest) => Promise<unknown>
  loading: boolean
}

export function VpsPane({ onDeploy, loading }: VpsPaneProps) {
  const [ip, setIp] = useState('')
  const [password, setPassword] = useState('')
  const [deploying, setDeploying] = useState(false)
  const [error, setError] = useState('')
  const [errorSummary, ...errorDetails] = error.split('\n\n')
  const busy = loading || deploying
  const canDeploy = ip.trim().length > 0 && password.length > 0

  const handleDeploy = async () => {
    if (!canDeploy || busy) return
    setDeploying(true)
    setError('')
    try {
      const result = await onDeploy({ ip: ip.trim(), password })
      if (result === false) setError('部署未完成，请检查 VPS 配置后重试。')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : '部署失败，请检查网络后重试。')
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
              disabled={busy}
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
              disabled={busy}
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
          密码仅用于本次部署，不会被保存。需允许 root 密码登录 SSH（端口 22），仅支持密钥登录的 VPS 暂不适用。支持使用 systemd 的常见 Linux 发行版及 Alpine/OpenRC，将自动补齐依赖并配置 Hysteria2。部署后请在安全组及防火墙放行 543/UDP。
        </div>
      </div>
      {error ? (
        <div className="vps-deploy-error" role="alert">
          <div className="vps-deploy-error-heading">
            <strong>部署未完成</strong>
            <button type="button" className="icon-button" aria-label="关闭部署错误" onClick={() => setError('')}>
              <X size={ICON.sm} />
            </button>
          </div>
          <div className="vps-deploy-error-message">{errorSummary}</div>
          {errorDetails.length > 0 ? (
            <details className="vps-deploy-error-details">
              <summary>查看错误详情</summary>
              <div className="vps-deploy-error-message">{errorDetails.join('\n\n')}</div>
            </details>
          ) : null}
        </div>
      ) : null}
      <Button
        tone="primary"
        icon={<Rocket size={ICON.sm} />}
        loading={busy}
        disabled={!canDeploy || busy}
        onClick={handleDeploy}
      >
        {busy ? '部署中，请稍候…' : '开始部署'}
      </Button>
    </div>
  )
}
