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

  const submitSub = async (event?: Event) => {
    event?.preventDefault()
    if (!subUrl().trim()) {
      props.showToast('请输入订阅链接', 'error')
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
      const urls = scanned.items.filter((item) => !item.already_added).map((item) => item.url)
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
    <main class="wizard-screen">
      <div class="wizard-sheet">
        <header class="wizard-header">
          <LogoIcon size={LOGO_SIZE.onboarding} />
          <div>
            <h1>初始化 Harbor</h1>
            <p class="wizard-sub">
              接入公共出海池订阅，或直接配置独享静态 IP 开启局域网终端与店铺分流。
            </p>
          </div>
        </header>

        <form class="wizard-section" onSubmit={(e) => { void submitSub(e) }}>
          <label class="field">
            <span>订阅链接 (HTTPS / HTTP)</span>
            <input
              data-autofocus
              aria-label="订阅链接"
              placeholder="https://example.com/api/v1/client/subscribe?token=..."
              value={subUrl()}
              disabled={props.pendingActions.has('addSub')}
              onInput={(e) => setSubUrl(e.currentTarget.value)}
            />
          </label>

          <div class="wizard-actions">
            <Button
              type="submit"
              tone="primary"
              loading={props.pendingActions.has('addSub')}
              icon={<PlusIcon size={ICON.sm} />}
            >
              接入订阅并启动网关
            </Button>
            <Button
              tone="secondary"
              loading={scanning() || props.pendingActions.has('importVerge')}
              icon={<DownloadIcon size={ICON.sm} />}
              onClick={() => { void quickImportVerge() }}
            >
              从 Clash Verge 导入
            </Button>
          </div>
        </form>

        <div class="wizard-divider">或直接配置独享静态线路</div>

        <div class="wizard-section">
          <div class="wizard-actions">
            <Button
              tone="secondary"
              icon={<LockIcon size={ICON.sm} />}
              onClick={() => props.onOpenAddNode('socks-residential')}
            >
              录入静态住宅 SOCKS5
            </Button>
            <Button
              tone="secondary"
              icon={<RocketIcon size={ICON.sm} />}
              onClick={() => props.onOpenAddNode('vps')}
            >
              SSH 部署海外 VPS
            </Button>
          </div>
          <Show when={props.pendingActions.size > 0}>
            <p class="drawer-note" role="status" style={{ margin: '0' }}>
              正在校验并应用网关配置，请稍候…
            </p>
          </Show>
        </div>
      </div>
    </main>
  )
}
