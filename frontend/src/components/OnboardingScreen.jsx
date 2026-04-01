import { useState } from 'react'
import { Plus, Settings } from 'lucide-react'
import { Button, LogoIcon } from './ui.jsx'
import { validateSubscriptionUrl } from '../utils.js'

export function OnboardingScreen({ onAddSub, loadingAction, onOpenAddNode, showToast }) {
  const [subUrl, setSubUrl] = useState('')

  const isLoading = loadingAction === 'addSub'

  const handleAddSub = () => {
    if (isLoading) return
    const error = validateSubscriptionUrl(subUrl)
    if (error) {
      showToast(error, 'error')
      return
    }
    onAddSub(subUrl.trim())
  }

  return (
    <div className="onboarding">
      <div className="onboarding-card">
        <div className="onboarding-header">
          <LogoIcon size={40} />
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
              icon={<Plus size={12} />}
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
            icon={<Settings size={14} />}
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
