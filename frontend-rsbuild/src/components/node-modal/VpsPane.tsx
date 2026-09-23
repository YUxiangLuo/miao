import { useState } from 'react'
import { Rocket, X } from 'lucide-react'
import { ICON } from '../../tokens'
import { Button } from '../ui'
import { VpsErrorDetails } from './VpsErrorDetails'
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
  const [completed, setCompleted] = useState(false)
  const [errorSummary, ...errorDetails] = error.split('\n\n')
  const busy = loading || deploying
  const canDeploy = ip.trim().length > 0 && password.length > 0

  const handleDeploy = async () => {
    if (!canDeploy || busy) return
    setDeploying(true)
    setError('')
    setCompleted(false)
    try {
      const result = await onDeploy({ ip: ip.trim(), password })
      if (result === false) setError('部署未完成，请检查 VPS 配置后重试。')
      else {
        setCompleted(true)
        setPassword('')
      }
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
          需支持 root 密码登录 SSH（22 端口），密码不保存。支持 systemd Linux 和 Alpine/OpenRC，自动安装依赖及 Hysteria2。部署后需在安全组和防火墙放行 543/UDP。
        </div>
        <p className="vps-deploy-hint">关闭窗口不取消部署，结果见「VPS 部署记录」。部署期间勿刷新或关闭页面；断连后先核对远端状态再重试。</p>
        {completed && <p className="vps-deploy-hint" role="status">部署完成，可查看部署记录和节点列表。</p>}
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
              <summary>{errorDetails.some((part) => part.startsWith('解决办法：')) ? '查看解决办法与错误详情' : '查看错误详情'}</summary>
              <VpsErrorDetails text={errorDetails.join('\n\n')} />
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
        {busy ? '部署中…' : '开始部署'}
      </Button>
    </div>
  )
}
