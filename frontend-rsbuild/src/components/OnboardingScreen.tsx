import { useState } from 'react'
import { Plus, Settings, Download, LoaderCircle } from 'lucide-react'
import { ICON, LOGO_SIZE } from '../tokens'
import { Button, LogoIcon } from './ui'
import { classNames, maskSubscription, validateSubscriptionUrl } from '../utils'
import type { ToastTone } from '../hooks/useApi'
import type { VergeImportItem, VergeImportResult } from '../types/api'

export interface OnboardingScreenProps {
  onAddSub: (url: string) => Promise<boolean>
  pendingActions: ReadonlySet<string>
  onOpenAddNode: () => void
  showToast: (message: string, tone?: ToastTone) => number
  onScanVerge: () => Promise<VergeImportResult | null>
  onImportVerge: (urls: string[]) => Promise<boolean>
}

export function OnboardingScreen({ onAddSub, pendingActions, onOpenAddNode, showToast, onScanVerge, onImportVerge }: OnboardingScreenProps) {
  const [subUrl, setSubUrl] = useState('')
  // null = 导入面板收起；扫描到订阅后才展开
  const [vergeItems, setVergeItems] = useState<VergeImportItem[] | null>(null)
  const [vergeSelected, setVergeSelected] = useState<ReadonlySet<string>>(new Set())

  const isLoading = pendingActions.has('addSub')
  const isScanning = pendingActions.has('scanVerge')
  const isImporting = pendingActions.has('importVerge')

  const handleAddSub = async () => {
    if (isLoading) return
    const url = subUrl.trim()
    const error = validateSubscriptionUrl(url)
    if (error) {
      showToast(error, 'error')
      return
    }
    const ok = await onAddSub(url)
    if (ok !== false) setSubUrl('')
  }

  const handleVergeScan = async () => {
    const result = await onScanVerge()
    if (!result || !result.found) {
      showToast('未检测到 Clash Verge Rev 的订阅', 'info')
      return
    }
    const importable = result.items.filter((item) => !item.already_added)
    if (importable.length === 0) {
      showToast('检测到的订阅都已在列表中', 'info')
      return
    }
    setVergeItems(result.items)
    setVergeSelected(new Set(importable.map((item) => item.url)))
  }

  const toggleVergeItem = (url: string) => {
    setVergeSelected((prev) => {
      const next = new Set(prev)
      if (next.has(url)) next.delete(url)
      else next.add(url)
      return next
    })
  }

  const handleVergeImport = async () => {
    if (!vergeItems || vergeSelected.size === 0) return
    const urls = vergeItems.filter((item) => vergeSelected.has(item.url)).map((item) => item.url)
    const ok = await onImportVerge(urls)
    if (ok) setVergeItems(null)
  }

  return (
    <div className="onboarding">
      <div className="onboarding-card">
        <div className="onboarding-header">
          <LogoIcon size={LOGO_SIZE.onboarding} />
          <h1 className="onboarding-title">Miao</h1>
          <p className="onboarding-subtitle">添加订阅链接或手动节点以开始使用</p>
        </div>

        <div className="onboarding-section">
          <div className="onboarding-input-row">
            <input
              value={subUrl}
              onChange={(e) => setSubUrl(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleAddSub()}
              placeholder="粘贴订阅链接..."
            />
            <Button
              tone="primary"
              size="sm"
              icon={<Plus size={ICON.xs} />}
              loading={isLoading}
              onClick={handleAddSub}
            >
              添加订阅
            </Button>
          </div>
        </div>

        <div className="onboarding-divider">
          <span>或</span>
        </div>

        <div className="onboarding-section">
          <Button
            tone="secondary"
            icon={<Settings size={ICON.sm} />}
            onClick={onOpenAddNode}
            className="onboarding-node-btn"
          >
            手动添加节点
          </Button>
        </div>

        <div className="onboarding-section onboarding-verge">
          {vergeItems === null ? (
            <button className="onboarding-verge-link" onClick={handleVergeScan} disabled={isScanning}>
              {isScanning ? <LoaderCircle size={ICON.xs} className="spin" /> : <Download size={ICON.xs} />}
              从 Clash Verge Rev 导入
            </button>
          ) : (
            <div className="onboarding-verge-panel">
              <div className="onboarding-verge-title">检测到 Clash Verge Rev 的 {vergeItems.length} 条订阅</div>
              <div className="onboarding-verge-list">
                {vergeItems.map((item) => (
                  <label
                    key={item.url}
                    className={classNames('onboarding-verge-item', item.already_added && 'added')}
                  >
                    <input
                      type="checkbox"
                      checked={item.already_added || vergeSelected.has(item.url)}
                      disabled={item.already_added || isImporting}
                      onChange={() => toggleVergeItem(item.url)}
                    />
                    <span className="onboarding-verge-name">{item.name || maskSubscription(item.url)}</span>
                    <span className="onboarding-verge-url">{maskSubscription(item.url)}</span>
                    {item.already_added && <span className="onboarding-verge-added">已添加</span>}
                  </label>
                ))}
              </div>
              <div className="onboarding-verge-actions">
                <Button tone="ghost" size="sm" onClick={() => setVergeItems(null)} disabled={isImporting}>
                  取消
                </Button>
                <Button
                  tone="primary"
                  size="sm"
                  loading={isImporting}
                  disabled={vergeSelected.size === 0}
                  onClick={handleVergeImport}
                >
                  导入 {vergeSelected.size} 条
                </Button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
