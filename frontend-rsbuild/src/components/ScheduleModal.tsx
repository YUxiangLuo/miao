import { For, Show, createEffect, createMemo, createSignal, createUniqueId } from 'solid-js'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { fetchJson } from '../hooks/request'
import { Button } from './ui'
import { classNames } from '../utils'
import { formatNextRun, formatTimezoneLabel } from '../scheduleFormat'
import type { ApiResponse, ScheduledRefreshRequest, ScheduledRefreshStatus } from '../types/api'
import { ClockIcon, PlusIcon, Trash2Icon, XIcon } from './icons'

export interface ScheduleModalProps {
  open: boolean
  saving: boolean
  onClose: () => void
  onSave: (request: ScheduledRefreshRequest) => Promise<boolean>
}

const MAX_TIMES = 24

function normalizedTimes(times: string[]): string[] {
  return [...new Set(times.map((time) => time.trim()).filter(Boolean))].sort()
}

export function ScheduleModal(props: ScheduleModalProps) {
  const titleId = createUniqueId()
  const dialogRef = useDialog(() => props.open, () => props.onClose())
  const [status, setStatus] = createSignal<ScheduledRefreshStatus | null>(null)
  const [loadError, setLoadError] = createSignal('')
  const [enabled, setEnabled] = createSignal(false)
  const [times, setTimes] = createSignal<string[]>([])
  const [submitting, setSubmitting] = createSignal(false)
  let generation = 0
  let loading = false
  let inFlight = false
  const busy = () => props.saving || submitting()

  const load = async () => {
    if (loading) return
    loading = true
    const currentGen = ++generation
    setLoadError('')
    try {
      const payload = await fetchJson<ApiResponse<ScheduledRefreshStatus>>('/api/scheduled-refresh')
      if (currentGen !== generation) return
      if (payload.success && payload.data) {
        setStatus(payload.data)
        setEnabled(Boolean(payload.data.enabled))
        setTimes(payload.data.times ?? [])
        setLoadError('')
      } else {
        setLoadError(payload.message || '加载定时刷新设置失败')
      }
    } catch (error) {
      if (currentGen === generation) setLoadError(error instanceof Error ? error.message : String(error))
    } finally {
      if (currentGen === generation) loading = false
    }
  }

  createEffect(
    () => props.open,
    (open) => {
      generation++
      loading = false
      if (!open) {
        setStatus(null)
        setLoadError('')
        setEnabled(false)
        setTimes([])
        return
      }
      setStatus(null)
      setLoadError('')
      void load()
    },
  )

  const draft = createMemo(() => normalizedTimes(times()))
  const canSave = () => !busy() && status() !== null && (!enabled() || draft().length > 0)
  const dirty = createMemo(() => {
    const current = status()
    if (!current) return false
    return enabled() !== current.enabled || draft().join(',') !== [...current.times].sort().join(',')
  })

  const updateTime = (index: number, value: string) => {
    setTimes((previous) => previous.map((time, i) => (i === index ? value : time)))
  }

  const submit = async () => {
    if (!canSave() || inFlight) return
    inFlight = true
    setSubmitting(true)
    const currentGen = generation
    try {
      if (await props.onSave({ enabled: enabled(), times: draft() }) && currentGen === generation) {
        props.onClose()
      }
    } finally {
      inFlight = false
      setSubmitting(false)
    }
  }

  return (
    <Show when={props.open}>
      <div class="modal-overlay drawer-overlay" onClick={props.onClose}>
        <div
          ref={dialogRef}
          class="modal-card drawer-panel schedule-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          tabindex={-1}
          onClick={(event) => event.stopPropagation()}
        >
          <div class="modal-title-row">
            <div class="modal-title-wrap">
              <ClockIcon size={ICON.lg} class="icon-accent" />
              <h3 id={titleId}>定时刷新</h3>
            </div>
            <button type="button" class="icon-button" onClick={props.onClose} aria-label="关闭定时刷新对话框">
              <XIcon size={ICON.md} />
            </button>
          </div>

          <Show when={busy()}>
            <p class="schedule-note schedule-save-status" role="status">保存中，关闭窗口不会取消请求。</p>
          </Show>

          <Show
            when={!loadError()}
            fallback={
              <div class="resource-error" role="alert">
                <strong>定时刷新设置读取失败</strong>
                <p>设置状态未知，不代表定时刷新已关闭。可重新读取，不会修改配置。</p>
                <Button tone="secondary" size="sm" onClick={load}>重新读取设置</Button>
                <details>
                  <summary>错误详情</summary>
                  <p>{loadError()}</p>
                </details>
              </div>
            }
          >
            <Show
              when={status()}
              fallback={<div class="empty-block" role="status">正在读取定时刷新设置…</div>}
            >
              {(currentStatus) => (
                <div class="schedule-body">
                  <button
                    type="button"
                    role="switch"
                    aria-checked={enabled() ? 'true' : 'false'}
                    aria-label="启用定时刷新"
                    class={classNames('toggle-switch', enabled() && 'on')}
                    disabled={busy()}
                    data-autofocus
                    onClick={() => setEnabled((value) => !value)}
                  >
                    <span class="toggle-switch-track">
                      <span class="toggle-switch-thumb" />
                    </span>
                    <span>{enabled() ? '已启用' : '未启用'}</span>
                  </button>

                  <div class="schedule-section-label">每天执行时间</div>
                  <div class="schedule-list">
                    <Show
                      when={times().length > 0}
                      fallback={<div class="schedule-empty">尚未添加时间</div>}
                    >
                      <For each={times()} keyed={false}>
                        {(time, index) => (
                          <div class="schedule-row">
                            <input
                              type="time"
                              value={time()}
                              step={60}
                              aria-label={`第 ${index + 1} 个执行时间`}
                              disabled={busy()}
                              onInput={(event) => updateTime(index, event.currentTarget.value)}
                              onChange={(event) => updateTime(index, event.currentTarget.value)}
                            />
                            <button
                              type="button"
                              class="icon-button subtle"
                              aria-label={`删除第 ${index + 1} 个执行时间`}
                              title="删除该时间点"
                              disabled={busy()}
                              onClick={() => setTimes((previous) => previous.filter((_, i) => i !== index))}
                            >
                              <Trash2Icon size={ICON.xs} />
                            </button>
                          </div>
                        )}
                      </For>
                    </Show>
                  </div>
                  <Button
                    tone="ghost"
                    size="sm"
                    icon={<PlusIcon size={ICON.xs} />}
                    disabled={busy() || times().length >= MAX_TIMES}
                    onClick={() => setTimes((previous) => [...previous, ''])}
                  >
                    添加时间
                  </Button>

                  <div class="schedule-note">
                    <div>
                      时间按系统时区执行：{formatTimezoneLabel(currentStatus().timezone, currentStatus().utc_offset)}
                    </div>
                    <Show when={currentStatus().enabled && currentStatus().next_run_at && !dirty()}>
                      <div>
                        下次自动刷新：{formatNextRun(currentStatus().next_run_at!, currentStatus().timezone, currentStatus().now)}
                      </div>
                    </Show>
                    <Show when={dirty() && enabled() && draft().length > 0}>
                      <div>保存后按新时间执行。</div>
                    </Show>
                    <Show when={enabled() && draft().length === 0}>
                      <div class="schedule-error">启用定时刷新至少需要一个时间点。</div>
                    </Show>
                  </div>
                </div>
              )}
            </Show>
          </Show>

          <div class="modal-actions">
            <Button tone="ghost" size="sm" onClick={props.onClose}>{busy() ? '关闭窗口' : '取消'}</Button>
            <Button
              tone="primary"
              size="sm"
              loading={busy()}
              disabled={!canSave()}
              onClick={submit}
            >
              保存
            </Button>
          </div>
        </div>
      </div>
    </Show>
  )
}
