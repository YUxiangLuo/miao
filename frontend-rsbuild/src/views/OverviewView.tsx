import { For, Show, createMemo } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { splitConnectionStats } from '../components/connectionFilters'
import { classNames, formatSpeed, formatUptime } from '../utils'
import { buildTerminalRows, terminalModeLabel } from '../domain/terminals'
import { buildAccessPortRows } from '../domain/accessPorts'
import { summarizeActiveTradePlatforms } from '../domain/tradeCatalog'
import type { useAppController } from '../hooks/useAppController'
import {
  ActivityIcon,
  ArrowRightIcon,
  GlobeIcon,
  LockIcon,
  MonitorSmartphoneIcon,
  ShieldAlertIcon,
  ShieldCheckIcon,
  StoreIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export function OverviewView(props: { app: AppController }) {
  const app = () => props.app

  const splitStats = createMemo(() => splitConnectionStats(app().connectionsInfo().connections))

  const terminalRows = createMemo(() => buildTerminalRows({
    terminals: app().terminals(),
    rules: app().rules(),
    connections: app().connectionsInfo().connections,
  }))

  const accessPortRows = createMemo(() => buildAccessPortRows({
    accessPorts: app().accessPorts(),
    nodes: app().nodes(),
    connections: app().connectionsInfo().connections,
  }))

  const activePlatforms = createMemo(() => summarizeActiveTradePlatforms(app().connectionsInfo().connections))

  const failClosedSeats = createMemo(() => terminalRows().filter((row) => row.failClosed))
  const failClosedPorts = createMemo(() => accessPortRows().filter((row) => row.blocked))
  const dedicatedSeatCount = createMemo(() => terminalRows().filter((row) => row.mode === 'dedicated').length)

  return (
    <>
      <Show when={failClosedSeats().length > 0 || failClosedPorts().length > 0}>
        <div class="hg-alert danger" role="alert">
          <div class="hg-alert-main">
            <ShieldAlertIcon size={ICON.lg} />
            <div>
              <strong>防关联离线熔断保护已生效（严防回退公共池导致封号）</strong>
              <p>
                当前有 {failClosedSeats().length} 个工位整机绑定与 {failClosedPorts().length} 个多店铺专属端口因目标独享专线离线已被自动阻断海外流量，国内办公直连不受影响。请检查对应专线或切换备用专线。
              </p>
            </div>
          </div>
          <div class="hg-inline-actions">
            <Show when={failClosedPorts().length > 0}>
              <Button tone="danger" size="sm" onClick={() => app().setActiveNav('access-ports')}>
                查看受阻店铺端口
              </Button>
            </Show>
            <Button tone="secondary" size="sm" onClick={() => app().setActiveNav('lines')}>
              排查独享专线
            </Button>
          </div>
        </div>
      </Show>

      <Show when={failClosedSeats().length === 0 && failClosedPorts().length === 0 && app().status().warnings.length > 0}>
        <div class="hg-alert warning" role="status">
          <div class="hg-alert-main">
            <ShieldAlertIcon size={ICON.lg} />
            <div>
              <strong>网关运行提醒</strong>
              <p>{app().status().warnings.map((w) => w.message).join('；')}</p>
            </div>
          </div>
        </div>
      </Show>

      <div class="hg-kpi-grid">
        <div class="hg-kpi-card">
          <div class="hg-kpi-top">
            <span>网关运行状态</span>
            <ShieldCheckIcon size={ICON.md} class="icon-accent" />
          </div>
          <div class="hg-kpi-value">
            {app().status().ready ? '跨境转发就绪' : app().status().initializing ? '正在启动…' : '转发已停止'}
          </div>
          <div class="hg-kpi-foot">
            <span class="num">在线时长 {formatUptime(app().status().uptime_secs)}</span>
            <span class={classNames('badge', app().status().route_mode === 'rule' ? 'emerald' : 'amber')}>
              {app().status().route_mode === 'rule' ? '大陆直连优先' : '全局出海模式'}
            </span>
          </div>
        </div>

        <div class="hg-kpi-card">
          <div class="hg-kpi-top">
            <span>实时跨境 / 国内分流吞吐</span>
            <ActivityIcon size={ICON.md} class="icon-accent" />
          </div>
          <div class="hg-kpi-value num">
            ↓ {formatSpeed((app().traffic().down ?? splitStats().proxy.downloadSpeed + splitStats().direct.downloadSpeed))}
          </div>
          <div class="hg-kpi-foot num">
            <span>海外通道 {splitStats().proxy.count} 连接</span>
            <span>国内直连 {splitStats().direct.count} 连接</span>
          </div>
        </div>

        <div class="hg-kpi-card">
          <div class="hg-kpi-top">
            <span>局域网办公工位终端</span>
            <MonitorSmartphoneIcon size={ICON.md} class="icon-accent" />
          </div>
          <div class="hg-kpi-value num">{terminalRows().length} 台</div>
          <div class="hg-kpi-foot">
            <span>已锁独享专线 {dedicatedSeatCount()} 台</span>
            <button type="button" class="btn btn-ghost btn-sm" onClick={() => app().setActiveNav('terminals')}>
              工位分配 <ArrowRightIcon size={12} />
            </button>
          </div>
        </div>

        <div class="hg-kpi-card">
          <div class="hg-kpi-top">
            <span>多店铺防关联资产</span>
            <StoreIcon size={ICON.md} class="icon-accent" />
          </div>
          <div class="hg-kpi-value num">
            {app().nodes().length} 专线 / {accessPortRows().length} 端口
          </div>
          <div class="hg-kpi-foot">
            <span>指纹浏览器单窗单 IP</span>
            <button type="button" class="btn btn-ghost btn-sm" onClick={() => app().setActiveNav('access-ports')}>
              管理端口 <ArrowRightIcon size={12} />
            </button>
          </div>
        </div>
      </div>

      <div class="hg-overview-grid">
        <section class="hg-card">
          <div class="hg-card-header">
            <div class="hg-card-title-group">
              <h2>
                <MonitorSmartphoneIcon size={ICON.md} class="icon-accent" />
                活跃办公工位与整机隔离态势
              </h2>
              <p>融合 DHCP 自动发现、员工备注与实时外贸访问速率</p>
            </div>
            <div class="hg-card-actions">
              <Button tone="secondary" size="sm" onClick={() => app().setActiveNav('terminals')}>
                全部工位 ({terminalRows().length})
              </Button>
            </div>
          </div>
          <Show
            when={terminalRows().length > 0}
            fallback={
              <div class="empty-block">
                当前尚未发现活跃局域网终端连接，可在「工位与终端」页预先登记固定 IP 办公电脑。
              </div>
            }
          >
            <div class="hg-table-wrap">
              <table class="hg-table">
                <thead>
                  <tr>
                    <th>办公工位 / IP</th>
                    <th>整机出口策略</th>
                    <th>活跃外贸业务</th>
                    <th>实时速率</th>
                  </tr>
                </thead>
                <tbody>
                  <For each={terminalRows().slice(0, 8)} keyed={(row) => row.ip}>
                    {(row) => (
                      <tr class={classNames(row().failClosed && 'row-danger')}>
                        <td>
                          <div class="hg-entity-primary">
                            <span>{row().displayName}</span>
                            <Show when={row().department}>
                              {(dept) => <span class="badge slate">{dept()}</span>}
                            </Show>
                          </div>
                          <div class="hg-entity-sub mono">{row().ip}</div>
                        </td>
                        <td>
                          <Show
                            when={!row().failClosed}
                            fallback={<span class="badge danger">⛔ 专线离线已熔断 ({row().target})</span>}
                          >
                            <span
                              class={classNames(
                                'badge',
                                row().mode === 'dedicated'
                                  ? 'emerald'
                                  : row().mode === 'block_overseas'
                                    ? 'amber'
                                    : 'ocean',
                              )}
                            >
                              {row().mode === 'dedicated' ? <LockIcon size={11} /> : null}
                              {row().mode === 'dedicated' && row().target
                                ? `独享 · ${row().target}`
                                : terminalModeLabel(row().mode)}
                            </span>
                          </Show>
                        </td>
                        <td>
                          <Show
                            when={row().activePlatforms.length > 0}
                            fallback={<span class="hg-entity-sub">常规办公 / 待机</span>}
                          >
                            <div class="hg-platform-tags">
                              <For each={row().activePlatforms} keyed={(p) => p.id}>
                                {(platform) => (
                                  <span class="badge slate">{platform().name}</span>
                                )}
                              </For>
                            </div>
                          </Show>
                        </td>
                        <td class="num">
                          <div>↓ {formatSpeed(row().downloadSpeed)}</div>
                          <div class="hg-entity-sub">{row().connectionCount} 条连接</div>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Show>
        </section>

        <section class="hg-card">
          <div class="hg-card-header">
            <div class="hg-card-title-group">
              <h2>
                <GlobeIcon size={ICON.md} class="icon-accent" />
                实时跨境业务平台矩阵
              </h2>
              <p>自动识别当前网关承载的外贸社媒、电商后台、收款与国内办公连接</p>
            </div>
            <div class="hg-card-actions">
              <Button tone="secondary" size="sm" onClick={() => app().setActiveNav('policies')}>
                配置业务分流
              </Button>
            </div>
          </div>
          <div class="hg-card-body">
            <Show
              when={activePlatforms().length > 0}
              fallback={
                <div class="empty-block">
                  当前暂无匹配已知外贸平台（WhatsApp / Amazon / LinkedIn / TikTok / 企微等）的活跃连接。
                </div>
              }
            >
              <div class="hg-platform-grid">
                <For each={activePlatforms()} keyed={(item) => item.platform.id}>
                  {(item) => (
                    <div class="hg-platform-card">
                      <div class="hg-platform-left">
                        <span
                          class="hg-platform-mark"
                          style={{ background: item().platform.accent }}
                        >
                          {item().platform.shortLabel}
                        </span>
                        <div style={{ 'min-width': '0' }}>
                          <div class="hg-platform-name">{item().platform.name}</div>
                          <div class="hg-platform-meta">
                            出口: {item().outbounds.slice(0, 2).join(', ')}
                          </div>
                        </div>
                      </div>
                      <div style={{ 'text-align': 'right' }} class="num">
                        <div style={{ 'font-weight': '600', 'font-size': '12.5px' }}>
                          ↓ {formatSpeed(item().downloadSpeed)}
                        </div>
                        <div class="hg-platform-meta">{item().connectionCount} 连接</div>
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </div>
        </section>
      </div>
    </>
  )
}
