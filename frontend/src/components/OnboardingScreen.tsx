import { useState } from 'react'
import { Plus, Settings } from 'lucide-react'
import { ICON, LOGO_SIZE } from '../tokens'
import { Button, LogoIcon } from './ui'
import { validateSubscriptionUrl } from '../utils'
import type { ToastTone } from '../hooks/useApi'

export interface OnboardingScreenProps {
  onAddSub: (url: string) => Promise<boolean>
  loadingAction: string
  onOpenAddNode: () => void
  showToast: (message: string, tone?: ToastTone) => number
}

export function OnboardingScreen({ onAddSub, loadingAction, onOpenAddNode, showToast }: OnboardingScreenProps) {
  const [subUrl, setSubUrl] = useState('')

  const isLoading = loadingAction === 'addSub'

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
      </div>
    </div>
  )
}
