import { For, Show, createMemo, createSignal, createUniqueId } from 'solid-js'
import { Portal } from '@solidjs/web'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import { isSingleSeatRule } from '../domain/terminals'
import { RULE_PRESETS, type RulePreset } from '../domain/rulePresets'
import { PROTOCOL_OPTIONS, ruleFieldLabel, ruleFieldOptions } from '../ruleFormat'
import { useDialog } from '../hooks/useDialog'
import { formatDelay } from '../utils'
import type { RuleInfo } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import { PlusIcon, RouteIcon, SearchIcon, Trash2Icon, XIcon } from '../components/icons'

type AppController = ReturnType<typeof useAppController>
type RuleScopeFilter = 'general' | 'all' | 'device'

const PRIORITY_STAGES: ReadonlyArray<{ index: string; label: string }> = [
  { index: '01', label: '独立代理端口' },
  { index: '02', label: '内网与大陆直连（规则分流模式）' },
  { index: '03', label: '自定义分流规则' },
  { index: '04', label: '默认代理出口' },
]

function describeRuleTarget(rule: RuleInfo): {
  tone: 'emerald' | 'cobalt' | 'crimson' | 'muted'
  text: string
} {
  if (rule.skipped) {
    const rawTarget = rule.target ? cleanNodeDisplay(rule.target).name : '未知节点'
    return { tone: 'crimson', text: `${rawTarget} · 节点不可用（已阻断）` }
  }
  if (!rule.target) {
    return { tone: 'muted', text: '自定义动作' }
  }
  if (rule.target === 'direct') {
    return { tone: 'emerald', text: '直连' }
  }
  if (rule.target === 'reject') {
    return { tone: 'crimson', text: '拦截' }
  }
  if (rule.target === 'proxy') {
    return { tone: 'cobalt', text: '默认代理' }
  }
  return { tone: 'emerald', text: cleanNodeDisplay(rule.target).name }
}

export function RulesView(props: { app: AppController }) {
  const app = () => props.app

  const [scopeFilter, setScopeFilter] = createSignal<RuleScopeFilter>('general')
  const [query, setQuery] = createSignal('')
  const [drawerOpen, setDrawerOpen] = createSignal(false)
  const [field, setField] = createSignal('domain_suffix')
  const [value, setValue] = createSignal('')
  const [target, setTarget] = createSignal('proxy')
  const [saving, setSaving] = createSignal(false)

  const dialogTitleId = createUniqueId()
  const dialogRef = useDialog<HTMLFormElement>(drawerOpen, () => setDrawerOpen(false))

  const deviceRuleCount = createMemo(() => app().rules().filter(isSingleSeatRule).length)
  const generalRuleCount = createMemo(() => app().rules().length - deviceRuleCount())

  const visibleRules = createMemo(() => {
    const scope = scopeFilter()
    const needle = query().trim().toLowerCase()
    return app().rules().filter((rule) => {
      const seat = isSingleSeatRule(rule)
      if (scope === 'general' && seat) return false
      if (scope === 'device' && !seat) return false
      if (!needle) return true
      const targetLabel = describeRuleTarget(rule).text.toLowerCase()
      return (
        (rule.value || '').toLowerCase().includes(needle)
        || (rule.field || '').toLowerCase().includes(needle)
        || (rule.target || '').toLowerCase().includes(needle)
        || rule.raw.toLowerCase().includes(needle)
        || targetLabel.includes(needle)
      )
    })
  })

  const manualNodes = createMemo(() => app().nodes())
  const manualTagSet = createMemo(() => new Set(manualNodes().map((n) => n.tag)))
  const subNodeTags = createMemo(() => (
    app().ruleNodeNames().filter((tag) => !manualTagSet().has(tag))
  ))

  const customRuleTargetOptions = createMemo<SelectOption[]>(() => {
    const delays = app().delays()
    const manualOpts: SelectOption[] = manualNodes().map((node) => ({
      value: node.tag,
      label: node.tag,
      group: '手动节点',
      badge: delays[node.tag] != null ? formatDelay(delays[node.tag]) : node.node_type.toUpperCase(),
      tone: 'emerald',
    }))
    const subOpts: SelectOption[] = subNodeTags().map((tag) => ({
      value: tag,
      label: tag,
      group: '订阅节点',
      badge: delays[tag] != null ? formatDelay(delays[tag]) : undefined,
      tone: 'cobalt',
    }))
    return [
      { value: 'proxy', label: '默认代理', tone: 'cobalt' },
      { value: 'direct', label: '直连', tone: 'emerald' },
      { value: 'reject', label: '拦截', tone: 'crimson' },
      ...manualOpts,
      ...subOpts,
    ]
  })

  const fieldOptions = createMemo(() => ruleFieldOptions(app().status().platform))

  const customRuleFieldOptions = createMemo<SelectOption[]>(() =>
    fieldOptions().map((opt) => ({
      value: opt.value,
      label: opt.label,
    })),
  )

  const protocolSelectOptions: SelectOption[] = PROTOCOL_OPTIONS.map((opt) => ({
    value: opt.value,
    label: opt.label.toUpperCase(),
  }))

  const fieldPlaceholder = createMemo(
    () => fieldOptions().find((opt) => opt.value === field())?.placeholder || '请输入匹配值',
  )

  const handleFieldChange = (nextField: string) => {
    setField(nextField)
    if (nextField === 'protocol') {
      if (!PROTOCOL_OPTIONS.some((p) => p.value === value())) {
        setValue('quic')
      }
    } else if (PROTOCOL_OPTIONS.some((p) => p.value === value())) {
      setValue('')
    }
  }

  const applyPreset = (preset: RulePreset) => {
    setField(preset.field)
    setValue(preset.value)
    setTarget(preset.recommendedTarget)
  }

  const submitCustomRule = async (event?: Event) => {
    event?.preventDefault()
    if (saving() || !value().trim()) return
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
      <header class="page-header">
        <div>
          <h1 class="page-title">分流规则</h1>
          <p class="page-subtitle">按域名、IP、端口或进程自定义流量出口</p>
        </div>
        <div class="page-actions">
          <Button
            tone="primary"
            size="sm"
            icon={<PlusIcon size={ICON.xs} />}
            onClick={() => setDrawerOpen(true)}
          >
            新建规则
          </Button>
        </div>
      </header>

      <div class="priority-strip" aria-label="路由匹配顺序">
        <span class="priority-prefix">匹配顺序</span>
        <For each={PRIORITY_STAGES}>
          {(stage, idx) => (
            <>
              <Show when={idx() > 0}>
                <span class="priority-sep" aria-hidden="true">→</span>
              </Show>
              <span class="priority-step">
                <span class="mono-tag num">{stage.index}</span>
                <span>{stage.label}</span>
              </span>
            </>
          )}
        </For>
      </div>

      <section class="surface-box" aria-label="自定义规则列表">
        <div class="surface-header">
          <div class="seg-rail" role="group" aria-label="规则范围筛选">
            <button
              type="button"
              class={['seg-btn', { active: scopeFilter() === 'general' }]}
              aria-pressed={scopeFilter() === 'general' ? 'true' : 'false'}
              onClick={() => setScopeFilter('general')}
            >
              常规规则 <span class="num">({generalRuleCount()})</span>
            </button>
            <button
              type="button"
              class={['seg-btn', { active: scopeFilter() === 'all' }]}
              aria-pressed={scopeFilter() === 'all' ? 'true' : 'false'}
              onClick={() => setScopeFilter('all')}
            >
              全部 <span class="num">({app().rules().length})</span>
            </button>
            <Show when={deviceRuleCount() > 0}>
              <button
                type="button"
                class={['seg-btn', { active: scopeFilter() === 'device' }]}
                aria-pressed={scopeFilter() === 'device' ? 'true' : 'false'}
                onClick={() => setScopeFilter('device')}
              >
                设备规则 <span class="num">({deviceRuleCount()})</span>
              </button>
            </Show>
          </div>

          <div class="studio-search">
            <SearchIcon size={ICON.sm} />
            <input
              type="search"
              aria-label="搜索规则"
              placeholder="搜索域名、IP 或出口…"
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
            />
          </div>
        </div>

        <Show
          when={visibleRules().length > 0}
          fallback={
            <div class="empty-state">
              {app().rules().length === 0
                ? '暂无自定义分流规则，点击右上角「新建规则」添加。'
                : scopeFilter() === 'general' && deviceRuleCount() > 0 && !query().trim()
                  ? `当前仅有 ${deviceRuleCount()} 条局域网设备绑定规则，可切换到「全部」或前往「设备与端口」查看。`
                  : '没有匹配当前筛选条件的规则。'}
            </div>
          }
        >
          <div class="hairline-list">
            <For each={visibleRules()} keyed={(rule) => `${rule.index}:${rule.raw}`}>
              {(rule) => {
                const targetState = () => describeRuleTarget(rule())
                return (
                  <div class={['hairline-row', { 'danger-tint': Boolean(rule().skipped) }]}>
                    <div class="row-main">
                      <div class="row-title">
                        <span class="mono-tag num">#{rule().index + 1}</span>
                        <span class="mono-tag">
                          {rule().field ? ruleFieldLabel(rule().field!) : 'JSON'}
                        </span>
                        <span class="mono" style={{ 'word-break': 'break-all' }}>
                          {rule().value || rule().raw}
                        </span>
                      </div>
                    </div>

                    <div class="row-end">
                      <span class={['status-inline', { danger: Boolean(rule().skipped) }]}>
                        <span class={['status-dot', targetState().tone]} />
                        <span>{targetState().text}</span>
                      </span>
                      <button
                        type="button"
                        class="icon-button danger"
                        aria-label={`删除规则 #${rule().index + 1}`}
                        title="删除规则"
                        onClick={() => app().handleOpenDeleteRuleConfirm(rule())}
                      >
                        <Trash2Icon size={ICON.sm} />
                      </button>
                    </div>
                  </div>
                )
              }}
            </For>
          </div>
        </Show>
      </section>

      <Show when={drawerOpen()}>
        <Portal>
          <div class="modal-overlay drawer-overlay" onClick={() => setDrawerOpen(false)}>
            <form
              ref={dialogRef}
              class="modal-card drawer-panel"
              role="dialog"
              aria-modal="true"
              aria-labelledby={dialogTitleId}
              tabindex={-1}
              onClick={(e) => e.stopPropagation()}
              onSubmit={(e) => { void submitCustomRule(e) }}
            >
              <div class="modal-title-row">
                <div class="modal-title-wrap">
                  <RouteIcon size={ICON.lg} class="icon-accent" />
                  <h3 id={dialogTitleId}>新建分流规则</h3>
                </div>
                <button
                  type="button"
                  class="icon-button"
                  aria-label="关闭新建规则窗口"
                  onClick={() => setDrawerOpen(false)}
                >
                  <XIcon size={ICON.md} />
                </button>
              </div>

              <div class="field">
                <span>常用模板（可选）</span>
                <div class="preset-chips">
                  <For each={RULE_PRESETS}>
                    {(preset) => (
                      <button
                        type="button"
                        class={[
                          'preset-chip',
                          { active: field() === preset.field && value() === preset.value },
                        ]}
                        title={preset.hint}
                        onClick={() => applyPreset(preset)}
                      >
                        {preset.label}
                      </button>
                    )}
                  </For>
                </div>
              </div>

              <div class="form-grid single">
                <div class="field">
                  <span>匹配类型</span>
                  <SelectPopover
                    ariaLabel="匹配类型"
                    fullWidth
                    value={field()}
                    options={customRuleFieldOptions()}
                    searchableThreshold={12}
                    onChange={handleFieldChange}
                  />
                </div>
              </div>

              <div class="form-grid single">
                <Show
                  when={field() === 'protocol'}
                  fallback={
                    <label class="field">
                      <span>匹配值</span>
                      <input
                        data-autofocus
                        aria-label="匹配值"
                        placeholder={fieldPlaceholder()}
                        value={value()}
                        onInput={(e) => setValue(e.currentTarget.value)}
                      />
                    </label>
                  }
                >
                  <div class="field">
                    <span>嗅探协议</span>
                    <SelectPopover
                      ariaLabel="嗅探协议"
                      fullWidth
                      value={value() || 'quic'}
                      options={protocolSelectOptions}
                      searchableThreshold={12}
                      onChange={setValue}
                    />
                  </div>
                </Show>
              </div>

              <div class="form-grid single">
                <div class="field">
                  <span>目标出口</span>
                  <SelectPopover
                    ariaLabel="目标出口"
                    fullWidth
                    value={target()}
                    options={customRuleTargetOptions()}
                    searchPlaceholder="搜索节点…"
                    onChange={setTarget}
                  />
                </div>
              </div>

              <div class="modal-actions">
                <Button tone="ghost" size="sm" onClick={() => setDrawerOpen(false)}>
                  取消
                </Button>
                <Button
                  type="submit"
                  tone="primary"
                  size="sm"
                  loading={saving()}
                  disabled={!value().trim()}
                >
                  添加规则
                </Button>
              </div>
            </form>
          </div>
        </Portal>
      </Show>
    </>
  )
}
