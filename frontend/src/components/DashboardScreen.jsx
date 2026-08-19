import {
  TopBar,
  ProxyCard,
  NodesCard,
  SubsCard,
  RulesCard,
  HomeConnections,
  McpFloat,
  ConfirmModal,
  ConnectionsModal,
  NodeModal,
  ToastStack,
} from './index.js'
import { WifiOff } from 'lucide-react'
import { ICON } from '../tokens.js'

export function DashboardScreen({ app }) {
  return (
    <div className="shell">
      <main className="workspace">
        {app.backendUnreachable && (
          <div className="offline-banner" role="alert">
            <WifiOff size={ICON.sm} />
            <span>与后端服务的连接已断开，正在自动重试…</span>
          </div>
        )}

        <TopBar
          status={app.status}
          traffic={app.traffic}
          versionInfo={app.versionInfo}
          upgrading={app.upgrading}
          onUpgradeClick={app.handleUpgradeClick}
          loadingAction={app.loadingAction}
          onSetRouteMode={app.handleOpenSetRouteModeConfirm}
          onOpenConnections={app.handleOpenConnections}
          primaryGroup={app.primaryGroup}
          delays={app.delays}
          testingNodes={app.testingNodes}
          onTestDelay={app.handleTestDelay}
        />

        <div className="content-grid">
          <div className="left-column">
            <ProxyCard
              status={app.status}
              primaryGroup={app.primaryGroup}
              primaryGroupName={app.primaryGroupName}
              nodeProtocols={app.nodeProtocols}
              delays={app.delays}
              testingNodes={app.testingNodes}
              testingGroup={app.testingGroup}
              switchingNode={app.switchingNode}
              nodeSelectPending={app.loadingAction === 'nodeSelect'}
              onTestDelay={app.handleTestDelay}
              onTestGroupDelays={app.handleTestGroupDelays}
              onSwitchProxy={app.handleSwitchProxy}
              onSetNodeSelect={app.handleSetNodeSelect}
              onOpenAddNode={app.openNodeModal}
            />
          </div>

          <div className="right-column">
            <NodesCard
              nodes={app.nodes}
              isInitializing={app.status.initializing}
              onDeleteNode={app.handleOpenDeleteNodeConfirm}
              onOpenAddNode={app.openNodeModal}
            />

            <SubsCard
              subs={app.subs}
              newSubUrl={app.newSubUrl}
              setNewSubUrl={app.setNewSubUrl}
              loadingAction={app.loadingAction}
              onAddSub={app.handleAddSubscription}
              onDeleteSub={app.handleOpenDeleteSubConfirm}
              onRefreshSubs={app.handleRefreshSubscriptions}
              isInitializing={app.status.initializing}
            />

            <RulesCard
              rules={app.rules}
              isInitializing={app.status.initializing}
              loadingAction={app.loadingAction}
              onAddRule={app.handleAddRule}
              onDeleteRule={app.handleOpenDeleteRuleConfirm}
              nodeNames={app.ruleNodeNames}
              connections={app.connectionsInfo?.connections}
              platform={app.status.platform || 'linux'}
              delays={app.delays}
              testingNodes={app.testingNodes}
              onTestNodes={() => {
                // 仅服务运行时测速有意义；候选 = 手动节点 ∪ 代理组节点
                if (app.status.running && app.ruleNodeNames.length > 0) {
                  app.handleTestGroupDelays(app.primaryGroupName || 'proxy', app.ruleNodeNames)
                }
              }}
            />
          </div>
        </div>

        {app.isDesktop && (
          <HomeConnections
            status={app.status}
            data={app.connectionsInfo}
            onOpenAll={app.handleOpenConnections}
          />
        )}
      </main>

      <McpFloat
        enabled={Boolean(app.status.mcp)}
        pending={app.loadingAction === 'toggleMcp'}
        onToggle={app.handleToggleMcp}
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

      <ConnectionsModal
        open={app.showConnectionsModal}
        status={app.status}
        data={app.connectionsInfo}
        loading={app.connectionsLoading}
        error={app.connectionsError}
        onClose={() => app.setShowConnectionsModal(false)}
      />

      <ConfirmModal
        open={app.confirmState.open}
        title={app.confirmState.title}
        message={app.confirmState.message}
        onCancel={app.closeConfirm}
        onConfirm={() => {
          const action = app.confirmState.onConfirm
          app.closeConfirm()
          action?.()
        }}
      />
    </div>
  )
}
