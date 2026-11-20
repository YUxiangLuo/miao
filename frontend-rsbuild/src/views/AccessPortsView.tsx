import { For, Show, createMemo, createSignal, createUniqueId } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { useDialog } from '../hooks/useDialog'
import { classNames, formatSpeed } from '../utils'
import { buildAccessPortRows, suggestNextAccessPort, type AccessPortRow } from '../domain/accessPorts'
import type { useAppController } from '../hooks/useAppController'
import {
  CopyIcon,
  EditIcon,
  LockIcon,
  PlusIcon,
  ShieldAlertIcon,
  StoreIcon,
  Trash2Icon,
  XIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export function AccessPortsView(props: { app: AppController }) {
  const app = () => props.app

  const [drawerOpen, setDrawerOpen] = createSignal(false)
  const [editId, setEditId] = createSignal<string | undefined>(undefined)
  const [name, setName] = createSignal('')
  const [port, setPort] = createSignal<number>(20101)
  const [outbound, setOutbound] = createSignal('')
  const [username, setUsername] = createSignal('')
  const [password, setPassword] = createSignal('')
  const [saving, setSaving] = createSignal(false)

  const titleId = createUniqueId()
  const dialogRef = useDialog(drawerOpen, () => setDrawerOpen(false))

  const rows = createMemo(() => buildAccessPortRows({
    accessPorts: app().accessPorts(),
    nodes: app().nodes(),
    connections: app().connectionsInfo().connections,
  }))

  const dedicatedNodes = createMemo(() => app().nodes())
  const extraPoolNodes = createMemo(() => {
    const manualSet = new Set(dedicatedNodes().map((n) => n.tag))
    return app().ruleNodeNames().filter((tag) => !manualSet.has(tag))
  })

  const openCreateDrawer = () => {
    const nextPort = suggestNextAccessPort(app().accessPorts())
    const defaultOutbound = dedicatedNodes()[0]?.tag || extraPoolNodes()[0] || 'proxy'
    setEditId(undefined)
    setName('')
    setPort(nextPort)
    setOutbound(defaultOutbound)
    setUsername('')
    setPassword('')
    setDrawerOpen(true)
  }

  const openEditDrawer = (row: AccessPortRow) => {
    setEditId(row.id)
    setName(row.name)
    setPort(row.port)
    setOutbound(row.outbound)
    setUsername(row.username || '')
    setPassword(row.password || '')
    setDrawerOpen(true)
  }

  const copyString = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value)
      app().showToast(`已复制 ${label}：${value}`, 'success')
    } catch {
      app().showToast(`请手动复制：${value}`, 'info')
    }
  }

  const submitPort = async () => {
    if (saving()) return
    setSaving(true)
    try {
      const ok = await app().handleSaveAccessPort({
        id: editId(),
        name: name().trim(),
        port: Number(port()),
        outbound: outbound().trim(),
        username: username() || undefined,
        password: password() || undefined,
      })
      if (ok) setDrawerOpen(false)
    } finally {
      setSaving(false)
    }
  }

  const authMismatch = () => Boolean(username()) !== Boolean(password())

  return (
    <>
      <div class="hg-arch-banner">
        <div>
          <h2>单台电脑多店铺 / 多应用细粒度 IP 隔离方案（端网协同架构）</h2>
          <p>
            路由器整机透明网关只能按电脑 IP 分流，无法区分同一台电脑里的多个浏览器店铺。通过 HarborGate 在局域网开启<strong>独立混合代理端口（HTTP + SOCKS5 TCP/UDP）</strong>，即可让指纹浏览器（AdsPower / 比特浏览器）、Chrome 多用户分身、WhatsApp 桌面端各自绑定专属端口：上游昂贵的海外静态住宅 IP 密码 100% 封存在网关内不落员工电脑，且任一专线掉线即刻自动熔断该店铺端口，绝不串号漏回公共池。
          </p>
          <div class="hg-arch-steps">
            <span class="hg-arch-step">① 在「专线与线路池」录入静态住宅 SOCKS5（可选香港跳板中转）</span>
            <span class="hg-arch-step">② 在本页新建专属端口（如 :20101 → 美区一店）</span>
            <span class="hg-arch-step">③ 一键复制粘贴至指纹浏览器或客户端代理设置</span>
          </div>
        </div>
        <Show when={app().accessPortsSupported()}>
          <Button
            tone="primary"
            icon={<PlusIcon size={ICON.sm} />}
            onClick={openCreateDrawer}
          >
            新建店铺专属端口
          </Button>
        </Show>
      </div>

      <Show when={!app().accessPortsSupported()}>
        <div class="hg-alert warning" role="status">
          <div class="hg-alert-main">
            <ShieldAlertIcon size={ICON.lg} />
            <div>
              <strong>当前路由器尚未升级支持局域网多店铺专属端口（mixed 入站）</strong>
              <p>
                本功能需要搭载 HarborGate `miao-client-v4` 内核的网关版本。升级路由器二进制后即可按需开通 `:20101+` 局域网专属代理入口。
              </p>
            </div>
          </div>
        </div>
      </Show>

      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <StoreIcon size={ICON.md} class="icon-accent" />
              已开通的多店铺 / 多应用专属代理端口 ({rows().length})
            </h2>
            <p>每个端口同时兼容 SOCKS5（支持 UDP 语音/视频）与 HTTP CONNECT 协议，默认对内网开放且严格走指定出口。</p>
          </div>
          <div class="hg-card-actions">
            <Show when={dedicatedNodes().length === 0}>
              <Button tone="secondary" size="sm" onClick={() => app().openNodeModal('socks-residential')}>
                先去录入静态住宅 IP
              </Button>
            </Show>
          </div>
        </div>

        <Show
          when={rows().length > 0}
          fallback={
            <div class="empty-block">
              暂未创建多店铺专属端口。点击右上角「新建店铺专属端口」，即可为 Amazon、TikTok、Shopify 或独立 WhatsApp 客户端分配专属静态出口。
            </div>
          }
        >
          <div class="hg-table-wrap">
            <table class="hg-table">
              <thead>
                <tr>
                  <th>店铺 / 应用名称</th>
                  <th>局域网接入地址 (HTTP/SOCKS5)</th>
                  <th>锁定出口拓扑链路</th>
                  <th>防关联保护状态</th>
                  <th>实时流量</th>
                  <th>客户端一键填入 & 操作</th>
                </tr>
              </thead>
              <tbody>
                <For each={rows()} keyed={(row) => row.id}>
                  {(row) => (
                    <tr class={classNames(row().blocked && 'row-danger')}>
                      <td>
                        <div class="hg-entity-primary">
                          <span>{row().name}</span>
                          <Show when={row().has_auth}>
                            <span class="badge slate">带密码鉴权</span>
                          </Show>
                        </div>
                        <div class="hg-entity-sub mono">入站标签: {row().inboundTag}</div>
                      </td>

                      <td>
                        <div class="mono" style={{ 'font-weight': '700', 'font-size': '13.5px' }}>
                          {row().listenEndpoint}
                        </div>
                        <div class="hg-entity-sub">
                          {row().has_auth ? `账号: ${row().username}` : '局域网免密直填'}
                        </div>
                      </td>

                      <td>
                        <div class="topo-chain">
                          <span class="topo-node mono">:{row().port}</span>
                          <Show when={row().detourRelay}>
                            {(relay) => (
                              <>
                                <span class="topo-arrow">→</span>
                                <span class="topo-node relay">跳板 {relay()}</span>
                              </>
                            )}
                          </Show>
                          <span class="topo-arrow">→</span>
                          <span class="topo-node exit">{row().outbound}</span>
                        </div>
                      </td>

                      <td>
                        <Show
                          when={!row().blocked}
                          fallback={
                            <span class="badge danger">
                              ⛔ 专线离线 · 已熔断阻断
                            </span>
                          }
                        >
                          <span class="badge emerald">
                            <LockIcon size={11} /> 独享受保护 (离线即断)
                          </span>
                        </Show>
                      </td>

                      <td class="num">
                        <div>↓ {formatSpeed(row().downloadSpeed)}</div>
                        <div class="hg-entity-sub">{row().connectionCount} 活跃连接</div>
                      </td>

                      <td>
                        <div class="hg-inline-actions">
                          <Button
                            tone="secondary"
                            size="sm"
                            icon={<CopyIcon size={ICON.xs} />}
                            title="复制 AdsPower / 比特浏览器 批量粘贴格式 (主机:端口[:账号:密码])"
                            onClick={() => { void copyString(row().browserPasteText, '指纹浏览器代理配置') }}
                          >
                            指纹浏览器格式
                          </Button>
                          <Button
                            tone="ghost"
                            size="sm"
                            onClick={() => { void copyString(row().socks5Uri, 'SOCKS5 链接') }}
                          >
                            SOCKS5
                          </Button>
                          <Button
                            tone="ghost"
                            size="sm"
                            onClick={() => { void copyString(row().httpUri, 'HTTP 代理') }}
                          >
                            HTTP
                          </Button>
                          <button
                            type="button"
                            class="icon-button"
                            aria-label={`编辑专属端口 ${row().name}`}
                            title="编辑端口与绑定专线"
                            onClick={() => openEditDrawer(row())}
                          >
                            <EditIcon size={ICON.sm} />
                          </button>
                          <button
                            type="button"
                            class="icon-button danger"
                            aria-label={`删除专属端口 ${row().name}`}
                            title="移除专属端口"
                            onClick={() => app().handleOpenDeleteAccessPortConfirm(row())}
                          >
                            <Trash2Icon size={ICON.sm} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </section>

      <Show when={drawerOpen()}>
        <div class="modal-overlay drawer-overlay" onClick={() => setDrawerOpen(false)}>
          <div
            ref={dialogRef}
            class="modal-card drawer-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            tabindex={-1}
            onClick={(e) => e.stopPropagation()}
          >
            <div class="modal-title-row">
              <div class="modal-title-wrap">
                <StoreIcon size={ICON.lg} class="icon-accent" />
                <h3 id={titleId}>{editId() ? '编辑店铺专属代理端口' : '新建多店铺专属代理端口'}</h3>
              </div>
              <button
                type="button"
                class="icon-button"
                aria-label="关闭专属端口抽屉"
                onClick={() => setDrawerOpen(false)}
              >
                <XIcon size={ICON.md} />
              </button>
            </div>

            <p class="drawer-help">
              进入此端口的所有网站与 DNS 请求都将强制从所选专线出口发出（不受电脑整机规则或大陆分流影响）；若目标专线或其前置跳板不可用，该端口会自动阻断连接以保护账号安全。
            </p>

            <div class="form-grid two">
              <label class="field">
                <span>店铺 / 业务窗口名称</span>
                <input
                  data-autofocus
                  placeholder="例如：Amazon 美区一店"
                  value={name()}
                  disabled={saving()}
                  onInput={(e) => setName(e.currentTarget.value)}
                />
              </label>

              <label class="field">
                <span>局域网监听端口 (推荐 20101+)</span>
                <input
                  type="number"
                  min={1024}
                  max={65535}
                  value={port()}
                  disabled={saving()}
                  onInput={(e) => setPort(Number(e.currentTarget.value || 0))}
                />
              </label>
            </div>

            <div class="form-grid single">
              <label class="field">
                <span>锁定出口专线</span>
                <select
                  aria-label="锁定出口专线"
                  value={outbound()}
                  disabled={saving()}
                  onChange={(e) => setOutbound(e.currentTarget.value)}
                >
                  <Show when={dedicatedNodes().length > 0}>
                    <optgroup label="🔒 独享静态专线 (推荐用于多店铺防关联)">
                      <For each={dedicatedNodes()}>
                        {(node) => (
                          <option value={node.tag}>
                            {node.tag} ({node.node_type}{node.detour ? ` · 经 ${node.detour} 中转` : ''})
                          </option>
                        )}
                      </For>
                    </optgroup>
                  </Show>
                  <Show when={extraPoolNodes().length > 0}>
                    <optgroup label="公共池单节点">
                      <For each={extraPoolNodes()}>
                        {(tag) => <option value={tag}>{tag}</option>}
                      </For>
                    </optgroup>
                  </Show>
                  <optgroup label="网关内置通道">
                    <option value="proxy">🌐 公共出海线路池 (动态轮换，勿用于敏感店铺)</option>
                    <option value="direct">⚡ 本地宽带直连</option>
                  </optgroup>
                </select>
              </label>
            </div>

            <div class="form-grid two">
              <label class="field">
                <span>内网连接账号（可选，防其他员工误连）</span>
                <input
                  autocomplete="off"
                  placeholder="留空则内网免密使用"
                  value={username()}
                  disabled={saving()}
                  onInput={(e) => setUsername(e.currentTarget.value)}
                />
              </label>

              <label class="field">
                <span>内网连接密码（可选）</span>
                <input
                  type="password"
                  autocomplete="new-password"
                  placeholder="与账号同时填写或同时留空"
                  value={password()}
                  disabled={saving()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                />
              </label>
            </div>

            <Show when={authMismatch()}>
              <p class="field-error" role="alert">内网鉴权账号与密码需同时填写，或同时留空。</p>
            </Show>

            <div class="modal-actions">
              <Button tone="ghost" size="sm" onClick={() => setDrawerOpen(false)}>取消</Button>
              <Button
                tone="primary"
                size="sm"
                loading={saving()}
                disabled={!name().trim() || port() < 1024 || port() > 65535 || !outbound() || authMismatch()}
                onClick={() => { void submitPort() }}
              >
                {editId() ? '保存修改' : '创建并启用端口'}
              </Button>
            </div>
          </div>
        </div>
      </Show>
    </>
  )
}
