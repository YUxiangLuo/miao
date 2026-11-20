import { Match, Switch } from 'solid-js'
import { LogoIcon, ToastStack } from './components/ui'
import { NodeModal } from './components/node-modal/NodeModal'
import { VpsTasks } from './components/VpsTasks'
import { AppShell } from './components/AppShell'
import { SetupWizard } from './views/SetupWizard'
import { useAppController } from './hooks/useAppController'

export type AppScreenMode = 'loading' | 'backend-unreachable' | 'onboarding' | 'dashboard'

export function resolveAppScreenMode(state: {
  firstLoadDone: boolean
  statusLoaded: boolean
  needsOnboarding: boolean
}): AppScreenMode {
  if (!state.firstLoadDone) return 'loading'
  if (!state.statusLoaded) return 'backend-unreachable'
  if (state.needsOnboarding) return 'onboarding'
  return 'dashboard'
}

export default function App() {
  const app = useAppController()
  const screenMode = () => resolveAppScreenMode({
    firstLoadDone: app.firstLoadDone(),
    statusLoaded: app.statusLoaded(),
    needsOnboarding: app.needsOnboarding(),
  })

  return (
    <Switch>
      <Match when={screenMode() === 'loading'}>
        <div class="shell-state-screen">
          <div class="shell-state-card" role="status">
            <LogoIcon size={40} />
            <strong>正在连接 Harbor 网关…</strong>
            <span class="shell-state-sub">正在同步终端、专属端口与全球线路状态</span>
          </div>
        </div>
      </Match>

      <Match when={screenMode() === 'backend-unreachable'}>
        <div class="shell-state-screen">
          <div class="shell-state-card" role="status">
            <LogoIcon size={40} />
            <strong>网关控制面暂时离线，正在重连…</strong>
            <span class="shell-state-sub">请确认路由器或 Harbor 服务进程处于运行状态</span>
          </div>
        </div>
      </Match>

      <Match when={screenMode() === 'onboarding'}>
        <SetupWizard
          onAddSub={app.handleOnboardingAddSub}
          pendingActions={app.pendingActions()}
          onOpenAddNode={app.openNodeModal}
          showToast={app.showToast}
          onScanVerge={app.scanClashVerge}
          onImportVerge={app.importClashVergeSubs}
        />
        <VpsTasks tasks={app.vpsTasks()} />
        <ToastStack toasts={app.toasts()} onDismiss={app.dismissToast} />
        <NodeModal
          open={app.showNodeModal()}
          nodeType={app.nodeType()}
          setNodeType={app.setNodeType}
          form={app.nodeForm()}
          setForm={app.setNodeForm}
          loading={app.vpsDeploying() || ['addNode', 'importNodes'].some((action) => app.pendingActions().has(action))}
          onClose={app.closeNodeModal}
          onSubmit={app.handleAddNode}
          onImport={app.handleImportNodes}
          onDeployVps={app.handleDeployVps}
          vpsSupported={app.status().vps_supported !== false}
          relayCandidates={app.ruleNodeNames()}
        />
      </Match>

      <Match when={screenMode() === 'dashboard'}>
        <AppShell app={app} />
      </Match>
    </Switch>
  )
}
