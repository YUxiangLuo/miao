import { Show, createMemo, createSignal } from 'solid-js'
import { ICON } from '../../tokens'
import { Button } from '../ui'
import { VpsErrorDetails } from './VpsErrorDetails'
import type { VpsDeployRequest } from '../../types/api'
import { RocketIcon, XIcon } from '../icons'

export interface VpsPaneProps {
  onDeploy: (req: VpsDeployRequest) => Promise<unknown>
  loading: boolean
}

export function VpsPane(props: VpsPaneProps) {
  const [ip, setIp] = createSignal('')
  const [password, setPassword] = createSignal('')
  const [deploying, setDeploying] = createSignal(false)
  const [error, setError] = createSignal('')
  const [completed, setCompleted] = createSignal(false)

  const errorParts = createMemo(() => {
    const [summary, ...details] = error().split('\n\n')
    return { summary, details }
  })
  const busy = () => props.loading || deploying()
  const canDeploy = () => ip().trim().length > 0 && password().length > 0

  const handleDeploy = async () => {
    if (!canDeploy() || busy()) return
    setDeploying(true)
    setError('')
    setCompleted(false)
    try {
      const result = await props.onDeploy({ ip: ip().trim(), password: password() })
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
    <div class="node-pane">
      <div class="node-pane-scroll">
        <div class="form-grid single">
          <label class="field">
            <span>VPS IP 地址</span>
            <input
              disabled={busy()}
              value={ip()}
              onInput={(event) => setIp(event.currentTarget.value)}
              placeholder="203.0.113.10"
              aria-label="VPS IP 地址"
            />
          </label>
        </div>
        <div class="form-grid single">
          <label class="field">
            <span>root 密码</span>
            <input
              disabled={busy()}
              type="password"
              autocomplete="new-password"
              value={password()}
              onInput={(event) => setPassword(event.currentTarget.value)}
              placeholder="root 登录密码"
              aria-label="root 密码"
            />
          </label>
        </div>
        <div class="vps-deploy-hint">
          需支持 root 密码登录 SSH（22 端口），密码不保存。支持 systemd Linux 和 Alpine/OpenRC，自动安装依赖及 Hysteria2。部署后需在安全组和防火墙放行 543/UDP。
        </div>
        <p class="vps-deploy-hint">关闭窗口不取消部署，结果见「VPS 部署记录」。部署期间勿刷新或关闭页面；断连后先核对远端状态再重试。</p>
        <Show when={completed()}>
          <p class="vps-deploy-hint" role="status">部署完成，可查看部署记录和节点列表。</p>
        </Show>
      </div>
      <Show when={error()}>
        <div class="vps-deploy-error" role="alert">
          <div class="vps-deploy-error-heading">
            <strong>部署未完成</strong>
            <button type="button" class="icon-button" aria-label="关闭部署错误" onClick={() => setError('')}>
              <XIcon size={ICON.sm} />
            </button>
          </div>
          <div class="vps-deploy-error-message">{errorParts().summary}</div>
          <Show when={errorParts().details.length > 0}>
            <details class="vps-deploy-error-details">
              <summary>
                {errorParts().details.some((part) => part.startsWith('解决办法：'))
                  ? '查看解决办法与错误详情'
                  : '查看错误详情'}
              </summary>
              <VpsErrorDetails text={errorParts().details.join('\n\n')} />
            </details>
          </Show>
        </div>
      </Show>
      <Button
        tone="primary"
        icon={<RocketIcon size={ICON.sm} />}
        loading={busy()}
        disabled={!canDeploy() || busy()}
        onClick={handleDeploy}
      >
        {busy() ? '部署中…' : '开始部署'}
      </Button>
    </div>
  )
}
