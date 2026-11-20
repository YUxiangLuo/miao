import { Show, createSignal } from 'solid-js'
import { ICON, LOGO_SIZE } from '../tokens'
import { Button, LogoIcon } from '../components/ui'
import type { ToastTone } from '../hooks/useApi'
import type { VergeImportResult } from '../types/api'
import { DownloadIcon, LockIcon, PlusIcon, RocketIcon } from '../components/icons'

export interface SetupWizardProps {
  onAddSub: (url: string) => Promise<boolean>
  pendingActions: ReadonlySet<string>
  onOpenAddNode: (preset?: 'socks-residential' | 'vps' | 'link') => void
  showToast: (message: string, tone?: ToastTone) => number
  onScanVerge: () => Promise<VergeImportResult | null>
  onImportVerge: (urls: string[]) => Promise<boolean>
}

export function SetupWizard(props: SetupWizardProps) {
  const [subUrl, setSubUrl] = createSignal('')
  const [scanning, setScanning] = createSignal(false)

  const submitSub = async (e?: Event) => {
    e?.preventDefault()
    if (!subUrl().trim()) {
      props.showToast('请输入线路池 HTTPS 订阅地址', 'error')
      return
    }
    if (await props.onAddSub(subUrl())) {
      setSubUrl('')
    }
  }

  const quickImportVerge = async () => {
    if (scanning()) return
    setScanning(true)
    try {
      const scanned = await props.onScanVerge()
      if (!scanned?.found || !scanned.items.length) {
        props.showToast('未检测到本机 Clash Verge 订阅配置', 'info')
        return
      }
      const urls = scanned.items.filter((i) => !i.already_added).map((i) => i.url)
      if (!urls.length) {
        props.showToast('本机 Clash Verge 订阅均已添加', 'info')
        return
      }
      await props.onImportVerge(urls)
    } finally {
      setScanning(false)
    }
  }

  return (
    <main class="hg-wizard-wrap">
      <div class="hg-wizard-container">
        <header class="hg-wizard-hero">
          <LogoIcon size={LOGO_SIZE.onboarding} />
          <div>
            <h1>欢迎启用 HarborGate · 海港网关</h1>
            <p>外贸企业跨境网络控制台 · 多店铺静态住宅 IP 防关联与国内办公零干扰分流</p>
          </div>
        </header>

        <div class="hg-wizard-cards">
          <section class="hg-card">
            <div class="hg-card-header">
              <div class="hg-card-title-group">
                <h2>方式一：接入公共出海线路池订阅（推荐首步）</h2>
                <p>用于团队日常海外网页检索、AI 助手，并作为海外静态住宅 SOCKS5 的高速前置跳板池</p>
              </div>
            </div>
            <form class="hg-card-body" onSubmit={(e) => { void submitSub(e) }} style={{ display: 'flex', 'flex-direction': 'column', gap: '14px' }}>
              <label class="field">
                <span>企业线路源订阅链接 (HTTPS / HTTP)</span>
                <input
                  data-autofocus
                  placeholder="https://example.com/api/v1/client/subscribe?token=..."
                  value={subUrl()}
                  disabled={props.pendingActions.has('addSub')}
                  onInput={(e) => setSubUrl(e.currentTarget.value)}
                />
              </label>

              <div class="hg-inline-actions">
                <Button
                  type="submit"
                  tone="primary"
                  loading={props.pendingActions.has('addSub')}
                  icon={<PlusIcon size={ICON.sm} />}
                >
                  保存订阅并初始化网关
                </Button>
                <Button
                  tone="secondary"
                  loading={scanning() || props.pendingActions.has('importVerge')}
                  icon={<DownloadIcon size={ICON.sm} />}
                  onClick={() => { void quickImportVerge() }}
                >
                  从 Clash Verge 一键导入
                </Button>
              </div>
            </form>
          </section>

          <section class="hg-card">
            <div class="hg-card-header">
              <div class="hg-card-title-group">
                <h2>方式二：直接录入独享静态专线 / 部署 VPS</h2>
                <p>如果你已有海外云服务器或静态住宅 SOCKS5 代理，可直接录入后分配给员工工位或店铺端口</p>
              </div>
            </div>
            <div class="hg-card-body" style={{ display: 'flex', 'flex-direction': 'column', gap: '10px' }}>
              <Button
                tone="secondary"
                icon={<LockIcon size={ICON.sm} />}
                onClick={() => props.onOpenAddNode('socks-residential')}
              >
                录入静态住宅 SOCKS5 / 手动节点
              </Button>
              <Button
                tone="secondary"
                icon={<RocketIcon size={ICON.sm} />}
                onClick={() => props.onOpenAddNode('link')}
              >
                SSH 一键部署海外 VPS / 粘贴分享链接
              </Button>
              <Show when={props.pendingActions.size > 0}>
                <p class="drawer-note" role="status">正在提交配置并校验内核，请稍候…</p>
              </Show>
            </div>
          </section>
        </div>
      </div>
    </main>
  )
}
