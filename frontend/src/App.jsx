import {
  NodeModal,
  ToastStack,
  OnboardingScreen,
} from './components/index.js'
import { DashboardScreen } from './components/DashboardScreen.jsx'
import { useAppController } from './hooks/useAppController.js'

export default function App() {
  const app = useAppController()

  if (!app.firstLoadDone) {
    return <div className="shell"><div className="onboarding-loading">加载中…</div></div>
  }

  // 首载从未拿到过后端响应 = 后端不可达（没起/宕机），不能按默认空数据误判成引导页；
  // 轮询仍在后台继续，恢复后自动进入正常流程
  if (!app.statusLoaded) {
    return <div className="shell"><div className="onboarding-loading">无法连接后端，正在自动重试…</div></div>
  }

  if (app.needsOnboarding) {
    return (
      <div className="shell">
        <OnboardingScreen
          onAddSub={app.handleOnboardingAddSub}
          loadingAction={app.loadingAction}
          onOpenAddNode={app.openNodeModal}
          showToast={app.showToast}
        />
        <ToastStack toasts={app.toasts} onDismiss={app.dismissToast} />
        <NodeModal
          open={app.showNodeModal}
          nodeType={app.nodeType}
          setNodeType={app.setNodeType}
          form={app.nodeForm}
          setForm={app.setNodeForm}
          loading={app.loadingAction === 'addNode'}
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
