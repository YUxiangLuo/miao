import { For, Show, createMemo, createSignal, createUniqueId } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { useDialog } from '../hooks/useDialog'
import { RULE_FIELD_OPTIONS, ruleFieldLabel, ruleTargetLabel } from '../ruleFormat'
import { isSingleSeatRule } from '../domain/terminals'
import {
  findPlatformPolicyRule,
  TRADE_CATEGORIES,
  TRADE_PLATFORMS,
  type TradeCategoryId,
  type TradePlatform,
} from '../domain/tradeCatalog'
import { classNames } from '../utils'
import type { useAppController } from '../hooks/useAppController'
import {
  GlobeIcon,
  PlusIcon,
  RouteIcon,
  Trash2Icon,
  XIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export function PoliciesView(props: { app: AppController }) {
  const app = () => props.app

  const [activeCategory, setActiveCategory] = createSignal<TradeCategoryId>('social')
  const [showSeatRules, setShowSeatRules] = createSignal(false)

  const [drawerOpen, setDrawerOpen] = createSignal(false)
  const [field, setField] = createSignal('domain_suffix')
  const [value, setValue] = createSignal('')
  const [target, setTarget] = createSignal('proxy')
  const [saving, setSaving] = createSignal(false)

  const titleId = createUniqueId()
  const dialogRef = useDialog(drawerOpen, () => setDrawerOpen(false))

  const categoryPlatforms = createMemo(() => (
    TRADE_PLATFORMS.filter((item) => item.category === activeCategory())
  ))

  const visibleRules = createMemo(() => (
    showSeatRules()
      ? app().rules()
      : app().rules().filter((rule) => !isSingleSeatRule(rule))
  ))

  const dedicatedNodes = createMemo(() => app().nodes().map((n) => n.tag))
  const extraNodes = createMemo(() => app().ruleNodeNames().filter((tag) => !dedicatedNodes().includes(tag)))

  const handleAssignPlatform = async (platform: TradePlatform, nextTarget: string) => {
    const existing = findPlatformPolicyRule(platform, app().rules())
    if (!nextTarget) {
      if (existing) app().handleOpenDeleteRuleConfirm(existing)
      return
    }
    if (existing && existing.target === nextTarget) return
    if (existing) {
      await app().apiCall(
        'rules',
        { method: 'DELETE', body: JSON.stringify({ index: existing.index, raw: existing.raw }) },
        'deleteRule',
      )
    }
    await app().handleAddRule({
      field: 'domain_suffix',
      value: platform.primarySuffix,
      target: nextTarget,
    })
  }

  const submitCustomRule = async () => {
    if (saving()) return
    setSaving(true)
    try {
      const ok = await app().handleAddRule({
        field: field(),
        value: value().trim(),
        target: target(),
      })
      if (ok) {
        setValue('')
        setDrawerOpen(false)
      }
    } finally {
      setSaving(false)
    }
  }

  return (
    <>
      <div class="hg-priority-bar" aria-label="企业网关路由优先级">
        <div class="hg-priority-step">
          <div class="hg-priority-num">优先级 01</div>
          <div class="hg-priority-title">多店铺专属端口</div>
          <div class="hg-priority-desc">进入 :20101+ 端口流量强制走绑定静态 IP，离线即断</div>
        </div>
        <div class="hg-priority-step">
          <div class="hg-priority-num">优先级 02</div>
          <div class="hg-priority-title">国内办公与网银直连</div>
          <div class="hg-priority-desc">企微、飞书、ERP、税务及内网地址默认走本地宽带直连</div>
        </div>
        <div class="hg-priority-step">
          <div class="hg-priority-num">优先级 03</div>
          <div class="hg-priority-title">工位整机隔离策略</div>
          <div class="hg-priority-desc">按员工电脑或手机 IP 锁定独享专线或仅限国内办公</div>
        </div>
        <div class="hg-priority-step">
          <div class="hg-priority-num">优先级 04</div>
          <div class="hg-priority-title">外贸平台业务策略</div>
          <div class="hg-priority-desc">本页配置的域名、IP 段与端口定向分流规则</div>
        </div>
        <div class="hg-priority-step">
          <div class="hg-priority-num">优先级 05</div>
          <div class="hg-priority-title">公共出海线路池</div>
          <div class="hg-priority-desc">其余海外常规网页与工具默认经公共池智能出海</div>
        </div>
      </div>

      {/* Foreign Trade Platform Presets */}
      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <GlobeIcon size={ICON.md} class="icon-accent" />
              外贸业务平台一键定向分流库
            </h2>
            <p>为全公司公共网络环境下的高频外贸应用指定统一出口（单机多店铺账号请优先使用「多店铺专属端口」）。</p>
          </div>
          <div class="hg-segmented" role="group" aria-label="外贸平台分类">
            <For each={TRADE_CATEGORIES}>
              {(cat) => (
                <button
                  type="button"
                  class={classNames('hg-segment', activeCategory() === cat.id && 'active')}
                  aria-pressed={activeCategory() === cat.id ? 'true' : 'false'}
                  onClick={() => setActiveCategory(cat.id)}
                >
                  {cat.label}
                </button>
              )}
            </For>
          </div>
        </div>

        <div class="hg-card-body">
          <div class="hg-policy-preset-grid">
            <For each={categoryPlatforms()} keyed={(p) => p.id}>
              {(platform) => {
                const matchedRule = () => findPlatformPolicyRule(platform(), app().rules())
                return (
                  <div class="hg-platform-card hg-policy-preset-card">
                    <div class="hg-platform-left">
                      <span class="hg-platform-mark" style={{ background: platform().accent }}>
                        {platform().shortLabel}
                      </span>
                      <div style={{ 'min-width': '0', flex: '1' }}>
                        <div class="hg-policy-preset-title-row">
                          <div class="hg-platform-name">{platform().name}</div>
                          <span class="hg-entity-sub mono hg-policy-preset-domain">*.{platform().primarySuffix}</span>
                        </div>
                        <div class="hg-platform-meta">{platform().description}</div>
                      </div>
                    </div>
                    <select
                      class="hg-select hg-policy-preset-select"
                      aria-label={`${platform().name} 分流出口`}
                      value={matchedRule()?.target || ''}
                      disabled={app().pendingActions().has('addRule')}
                      onChange={(e) => { void handleAssignPlatform(platform(), e.currentTarget.value) }}
                    >
                      <option value="">
                        {platform().category === 'domestic' ? '默认（大陆规则集直连）' : '默认（跟随工位/公共池）'}
                      </option>
                      <option value="proxy">🌐 走公共出海线路池</option>
                      <option value="direct">⚡ 强制本地直连</option>
                      <option value="reject">⛔ 全网阻断访问</option>
                      <Show when={dedicatedNodes().length > 0}>
                        <optgroup label="🔒 指定独享静态专线">
                          <For each={dedicatedNodes()}>
                            {(tag) => <option value={tag}>🔒 专线 · {tag}</option>}
                          </For>
                        </optgroup>
                      </Show>
                      <Show when={extraNodes().length > 0}>
                        <optgroup label="指定公共池单节点">
                          <For each={extraNodes()}>
                            {(tag) => <option value={tag}>节点 · {tag}</option>}
                          </For>
                        </optgroup>
                      </Show>
                    </select>
                  </div>
                )
              }}
            </For>
          </div>
        </div>
      </section>

      {/* Custom Domain / IP / Port Rules Table */}
      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <RouteIcon size={ICON.md} class="icon-accent" />
              自定义域名 / IP 段 / 端口分流策略 ({visibleRules().length})
            </h2>
            <p>支持将自建独立站后台、海外 ERP、邮件服务器或特定网段精确指向指定出口。</p>
          </div>
          <div class="hg-card-actions">
            <label class="field checkbox-field" style={{ 'font-weight': '500' }}>
              <input
                type="checkbox"
                checked={showSeatRules()}
                onChange={(e) => setShowSeatRules(e.currentTarget.checked)}
              />
              <span>同时显示工位整机 IP 规则</span>
            </label>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={() => setDrawerOpen(true)}
            >
              新增自定义策略
            </Button>
          </div>
        </div>

        <Show
          when={visibleRules().length > 0}
          fallback={
            <div class="empty-block">
              暂无自定义域名或端口分流规则（工位整机 IP 规则请在「工位与终端」页集中管理，或勾选上方复选框查看）。
            </div>
          }
        >
          <div class="hg-table-wrap">
            <table class="hg-table">
              <thead>
                <tr>
                  <th>序号</th>
                  <th>匹配维度</th>
                  <th>匹配值</th>
                  <th>目标出口通道</th>
                  <th>生效状态</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                <For each={visibleRules()} keyed={(rule) => `${rule.index}:${rule.raw}`}>
                  {(rule) => (
                    <tr class={classNames(rule().skipped && 'row-danger')}>
                      <td class="num">#{rule().index + 1}</td>
                      <td>
                        <span class="badge slate">
                          {rule().field ? ruleFieldLabel(rule().field!) : '复合 JSON 规则'}
                        </span>
                      </td>
                      <td class="mono">{rule().value || rule().raw}</td>
                      <td>
                        <span
                          class={classNames(
                            'badge',
                            rule().target === 'direct'
                              ? 'emerald'
                              : rule().target === 'reject'
                                ? 'danger'
                                : 'ocean',
                          )}
                        >
                          {rule().target ? ruleTargetLabel(rule().target!) : '自定义动作'}
                        </span>
                      </td>
                      <td>
                        <Show
                          when={!rule().skipped}
                          fallback={<span class="badge danger">⛔ 出口缺失 · 已自动阻断防漏流</span>}
                        >
                          <span class="badge emerald">已生效</span>
                        </Show>
                      </td>
                      <td>
                        <button
                          type="button"
                          class="icon-button danger"
                          aria-label={`删除规则 #${rule().index + 1}`}
                          onClick={() => app().handleOpenDeleteRuleConfirm(rule())}
                        >
                          <Trash2Icon size={ICON.sm} />
                        </button>
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
                <RouteIcon size={ICON.lg} class="icon-accent" />
                <h3 id={titleId}>新增自定义业务分流策略</h3>
              </div>
              <button
                type="button"
                class="icon-button"
                aria-label="关闭策略抽屉"
                onClick={() => setDrawerOpen(false)}
              >
                <XIcon size={ICON.md} />
              </button>
            </div>

            <div class="form-grid single">
              <label class="field">
                <span>匹配条件类型</span>
                <select value={field()} onChange={(e) => setField(e.currentTarget.value)}>
                  <For each={RULE_FIELD_OPTIONS}>
                    {(opt) => <option value={opt.value}>{opt.label}</option>}
                  </For>
                </select>
              </label>
            </div>

            <div class="form-grid single">
              <label class="field">
                <span>匹配目标值</span>
                <input
                  data-autofocus
                  placeholder={
                    field().startsWith('domain')
                      ? '例如：my-b2b-shop.com'
                      : field().includes('ip')
                        ? '例如：192.168.8.0/24'
                        : '请输入匹配值'
                  }
                  value={value()}
                  onInput={(e) => setValue(e.currentTarget.value)}
                />
              </label>
            </div>

            <div class="form-grid single">
              <label class="field">
                <span>转发目标出口</span>
                <select value={target()} onChange={(e) => setTarget(e.currentTarget.value)}>
                  <option value="proxy">🌐 公共出海线路池</option>
                  <option value="direct">⚡ 本地宽带直连</option>
                  <option value="reject">⛔ 阻断拦截 (Reject)</option>
                  <Show when={dedicatedNodes().length > 0}>
                    <optgroup label="🔒 独享静态专线">
                      <For each={dedicatedNodes()}>
                        {(tag) => <option value={tag}>🔒 专线 · {tag}</option>}
                      </For>
                    </optgroup>
                  </Show>
                  <Show when={extraNodes().length > 0}>
                    <optgroup label="公共池单节点">
                      <For each={extraNodes()}>
                        {(tag) => <option value={tag}>{tag}</option>}
                      </For>
                    </optgroup>
                  </Show>
                </select>
              </label>
            </div>

            <div class="modal-actions">
              <Button tone="ghost" size="sm" onClick={() => setDrawerOpen(false)}>取消</Button>
              <Button
                tone="primary"
                size="sm"
                loading={saving()}
                disabled={!value().trim()}
                onClick={() => { void submitCustomRule() }}
              >
                添加策略
              </Button>
            </div>
          </div>
        </div>
      </Show>
    </>
  )
}
