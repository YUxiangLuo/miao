import {
  NodeModal,
  ToastStack,
  OnboardingScreen,
} from './components/index'
import { DashboardScreen } from './components/DashboardScreen'
import { VpsTasks } from './components/VpsTasks'
import { useAppController } from './hooks/useAppController'

export type AppScreenMode = 'loading' | 'backend-unreachable' | 'onboarding' | 'dashboard'

export function resolveAppScreenMode(state: {
  firstLoadDone: boolean
  statusLoaded: boolean
  needsOnboarding: boolean
}): AppScreenMode {
  if (!state.firstLoadDone) return 'loading'
  // 首载从未拿到过后端响应 = 后端不可达（没起/宕机），不能按默认空数据误判成引导页；
  // 轮询仍在后台继续，恢复后自动进入正常流程
  if (!state.statusLoaded) return 'backend-unreachable'
  if (state.needsOnboarding) return 'onboarding'
  return 'dashboard'
}

export default function App() {
  const app = useAppController()
  const screenMode = resolveAppScreenMode(app)

  if (screenMode === 'loading') {
    return <div className="shell"><div className="onboarding-loading" role="status">加载中…</div></div>
  }

  if (screenMode === 'backend-unreachable') {
    return <div className="shell"><div className="onboarding-loading" role="status">无法连接后端，正在自动重试…</div></div>
  }

  if (screenMode === 'onboarding') {
    return (
      <div className="shell">
        <OnboardingScreen
          onAddSub={app.handleOnboardingAddSub}
          pendingActions={app.pendingActions}
          onOpenAddNode={app.openNodeModal}
          showToast={app.showToast}
          onScanVerge={app.scanClashVerge}
          onImportVerge={app.importClashVergeSubs}
        />
        <VpsTasks tasks={app.vpsTasks} />
        <ToastStack toasts={app.toasts} onDismiss={app.dismissToast} />
        <NodeModal
          open={app.showNodeModal}
          nodeType={app.nodeType}
          setNodeType={app.setNodeType}
          form={app.nodeForm}
          setForm={app.setNodeForm}
          loading={app.vpsDeploying || ['addNode', 'importNodes'].some((action) => app.pendingActions.has(action))}
          onClose={app.closeNodeModal}
          onSubmit={app.handleAddNode}
          onImport={app.handleImportNodes}
          onDeployVps={app.handleDeployVps}
          vpsSupported={app.status.vps_supported !== false}
        />
      </div>
    )
  }

  return <DashboardScreen app={app} />
}
