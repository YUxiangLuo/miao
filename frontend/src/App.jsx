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
        />
      </div>
    )
  }

  return <DashboardScreen app={app} />
}
