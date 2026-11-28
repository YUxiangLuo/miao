import { Show, createSignal, onCleanup } from 'solid-js'
import { Portal } from '@solidjs/web'
import { ICON } from '../tokens'
import { API_HEADERS, DELAY_TEST_URL } from '../utils'
import { fetchJson } from '../hooks/request'
import { useDialog } from '../hooks/useDialog'
import { usePresence } from '../motion'
import { Button } from './ui'
import type { ApiResponse, ConnectivityResult, StatusData } from '../types/api'
import { ActivityIcon, XIcon } from './icons'

function describeResult(result: ConnectivityResult): string {
  if (result.success) {
    const status = result.http_status
    if (!status) return '收到响应但缺少状态码，无法判断访问结果。'
    if (status >= 400) return `HTTP ${status}：目标已响应，但访问未成功。请核对网址或访问限制；站点也可能不支持 HEAD。`
    return `HTTP ${status}：HEAD 请求完成，不保证页面或登录正常。`
  }
  if (result.error_kind === 'timeout') return '请求超时：5 秒内未完成。请检查网络，或换一个网址测试。'
  if (result.error_kind === 'connect') return '连接未建立，请检查地址和网络；暂无法区分 DNS、TLS 或目标服务问题。'
  return '请求未完成，请核对网址和错误详情。'
}

export function ConnectivityCheck(props: { status: StatusData; backendUnreachable: boolean }) {
  const [open, setOpen] = createSignal(false)
  const [panelEl, setPanelEl] = createSignal<HTMLElement | null>(null)
  const presence = usePresence({
    open,
    element: panelEl,
    exitDurationToken: 'fast',
  })
  const [url, setUrl] = createSignal(DELAY_TEST_URL)
  const [pending, setPending] = createSignal(false)
  const [validation, setValidation] = createSignal('')
  const [report, setReport] = createSignal('')
  const [summary, setSummary] = createSignal('')
  const [copyMessage, setCopyMessage] = createSignal('')
  let activeRequest: AbortController | null = null
  let reportEl: HTMLTextAreaElement | undefined
  const dialogRef = useDialog(open, () => setOpen(false))

  onCleanup(() => {
    activeRequest?.abort()
    activeRequest = null
  })

  const service = () => (
    props.backendUnreachable
      ? '后端不可达，服务状态未知'
      : props.status.ready
        ? '代理服务已就绪'
        : props.status.phase === 'stopped'
          ? '代理服务已停止'
          : '代理服务尚未就绪'
  )

  const runCheck = async () => {
    if (activeRequest) return
    const target = url().trim()
    try {
      const parsed = new URL(target)
      if (!['http:', 'https:'].includes(parsed.protocol)) throw new Error('unsupported protocol')
    } catch {
      setValidation('请输入完整的 HTTP 或 HTTPS 网址。')
      return
    }
    setValidation('')
    setCopyMessage('')
    setPending(true)
    const controller = new AbortController()
    activeRequest = controller
    const startedAt = new Date().toLocaleString()
    const startService = service()
    let conclusion: string
    let details = ''
    try {
      const response = await fetchJson<ApiResponse<ConnectivityResult>>('/api/connectivity', {
        method: 'POST',
        headers: API_HEADERS,
        body: JSON.stringify({ url: target }),
        signal: controller.signal,
      })
      if (!response.success || !response.data) throw new Error(response.message || '后端未返回检查结果')
      conclusion = describeResult(response.data)
      details = `耗时：${response.data.latency_ms == null ? '无测量结果' : `${response.data.latency_ms} ms`}\n错误类型：${response.data.error_kind || '无'}\n错误详情：${response.data.error || '无'}`
    } catch (error) {
      conclusion = '未取得检查结果，目标是否可达未知。请检查 Harbor 后端连接。'
      details = `错误详情：${error instanceof Error ? error.message : String(error)}`
    }
    if (activeRequest !== controller) return
    setSummary(conclusion)
    setReport(`Harbor 连接检查\n开始时间：${startedAt}\n完成时间：${new Date().toLocaleString()}\n检查开始时：${startService}\n目标：${target}\n方法：HEAD，超时 5 秒\n范围：Harbor 所在主机；不使用代理环境变量，仍可能经过系统 TUN/路由。不是指定节点或浏览器测试。\n结论：${conclusion}\n${details}`)
    activeRequest = null
    setPending(false)
  }

  const copyReport = async () => {
    try {
      await navigator.clipboard.writeText(report())
      setCopyMessage('已复制检查报告')
    } catch {
      reportEl?.focus()
      reportEl?.select()
      setCopyMessage('无法自动复制，已选中报告，请手动复制。')
    }
  }

  return (
    <>
      <Button tone="secondary" size="sm" icon={<ActivityIcon size={ICON.xs} />} onClick={() => setOpen(true)}>
        连接检查
      </Button>
      <Show when={presence.mounted()}>
        <Portal>
          <div
            class="modal-overlay drawer-overlay"
            data-motion="scrim"
            data-motion-state={presence.motionState()}
            onClick={() => setOpen(false)}
          >
            <div
              ref={(el) => {
                setPanelEl(el)
                dialogRef(el)
              }}
              class="modal-card drawer-panel connectivity-dialog"
              role="dialog"
              aria-modal="true"
              aria-label="连接检查"
              tabindex={-1}
              inert={!open() ? true : undefined}
              data-motion="slide-edge"
              data-side="right"
              data-motion-state={presence.motionState()}
              onClick={(event) => event.stopPropagation()}
            >
              <div class="modal-title-row">
                <div class="modal-title-wrap">
                  <ActivityIcon size={ICON.lg} class="icon-accent" />
                  <h3>连接检查</h3>
                </div>
                <button type="button" class="icon-button" aria-label="关闭连接检查" onClick={() => setOpen(false)}>
                  <XIcon size={ICON.sm} />
                </button>
              </div>
              <p class="drawer-status-line" role="status">{service()}</p>
              <p class="drawer-help">从 Harbor 主机发送 HEAD 请求，不修改配置。不使用代理环境变量，可能经过系统 TUN/路由；不测试浏览器或指定节点。</p>
              <label class="field">
                <span>目标网址</span>
                <input
                  data-autofocus
                  aria-label="目标网址"
                  value={url()}
                  disabled={pending()}
                  onInput={(event) => setUrl(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.isComposing || event.keyCode === 229) return
                    if (event.key === 'Enter') void runCheck()
                  }}
                />
              </label>
              <Show when={validation()}>
                {(msg) => <p class="field-error" role="alert">{msg()}</p>}
              </Show>
              <div class="drawer-inline-actions">
                <Button tone="primary" loading={pending()} onClick={runCheck}>开始检查</Button>
              </div>
              <p class="drawer-note">
                {pending() ? '检查中，关闭窗口后仍可返回查看结果。' : '手动检查，结果保留至下次检查完成或刷新页面。'}
              </p>
              <Show when={report()}>
                <section class="connectivity-report" aria-label="最近一次检查结果">
                  <p role="status">{pending() ? '以下为上一次结果：' : ''}{summary()}</p>
                  <textarea ref={reportEl} readonly rows={7} value={report()} aria-label="检查报告" />
                  <Button tone="secondary" size="sm" onClick={copyReport}>复制检查报告</Button>
                  <Show when={copyMessage()}>
                    {(msg) => <p class="drawer-note" role="status">{msg()}</p>}
                  </Show>
                </section>
              </Show>
            </div>
          </div>
        </Portal>
      </Show>
    </>
  )
}
