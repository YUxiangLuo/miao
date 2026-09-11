import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { Clock, Plus, Trash2, X } from 'lucide-react'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { Button } from './ui'
import { classNames } from '../utils'
import { formatNextRun, formatTimezoneLabel } from '../scheduleFormat'
import type { ApiResponse, ScheduledRefreshRequest, ScheduledRefreshStatus } from '../types/api'

export interface ScheduleModalProps {
  open: boolean
  saving: boolean
  onClose: () => void
  /** 返回是否保存成功；成功时由本组件负责关闭 */
  onSave: (request: ScheduledRefreshRequest) => Promise<boolean>
}

/** 与后端 `services::schedule::MAX_TIMES` 保持一致：超出时禁用添加入口。 */
const MAX_TIMES = 24

/** 合并本地草稿：去空、去重、按时间升序（与后端 normalize 同口径）。 */
function normalizedTimes(times: string[]): string[] {
  return [...new Set(times.map((time) => time.trim()).filter(Boolean))].sort()
}

// 定时刷新弹窗：开关 + 每天的执行时刻。时刻按后端系统时区解释与执行，
// 后端在响应里给出时区名/偏移和下次执行时间，面板不做本地时区换算。
export function ScheduleModal({ open, saving, onClose, onSave }: ScheduleModalProps) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [status, setStatus] = useState<ScheduledRefreshStatus | null>(null)
  const [loadError, setLoadError] = useState('')
  const [enabled, setEnabled] = useState(false)
  const [times, setTimes] = useState<string[]>([])
  const generationRef = useRef(0)

  const load = useCallback(async () => {
    const generation = ++generationRef.current
    try {
      const response = await fetch('/api/scheduled-refresh')
      const payload: ApiResponse<ScheduledRefreshStatus> = await response.json()
      if (generation !== generationRef.current) return
      if (payload.success && payload.data) {
        setStatus(payload.data)
        setEnabled(Boolean(payload.data.enabled))
        setTimes(payload.data.times ?? [])
        setLoadError('')
      } else {
        setLoadError(payload.message || '加载定时刷新设置失败')
      }
    } catch {
      if (generation === generationRef.current) setLoadError('加载定时刷新设置失败')
    }
  }, [])

  // 打开时加载；关闭时清空草稿，避免下次打开残留旧值
  useEffect(() => {
    generationRef.current += 1
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
  }, [open, load])

  if (!open) return null

  const draft = normalizedTimes(times)
  const canSave = !saving && status !== null && (!enabled || draft.length > 0)
  const dirty = status !== null
    && (enabled !== status.enabled || draft.join(',') !== [...status.times].sort().join(','))

  const updateTime = (index: number, value: string) => {
    setTimes((previous) => previous.map((time, i) => (i === index ? value : time)))
  }

  const submit = async () => {
    if (!canSave) return
    if (await onSave({ enabled, times: draft })) onClose()
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card schedule-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Clock size={ICON.lg} className="icon-accent" />
            <h3 id={titleId}>定时刷新</h3>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭定时刷新对话框">
            <X size={ICON.md} />
          </button>
        </div>

        {loadError ? (
          <div className="empty-block schedule-error">{loadError}</div>
        ) : status === null ? (
          <div className="empty-block">加载中…</div>
        ) : (
          <>
            <button
              type="button"
              role="switch"
              aria-checked={enabled}
              aria-label="启用定时刷新"
              className={classNames('toggle-switch', enabled && 'on')}
              disabled={saving}
              data-autofocus
              onClick={() => setEnabled((value) => !value)}
            >
              <span className="toggle-switch-track">
                <span className="toggle-switch-thumb" />
              </span>
              <span>{enabled ? '已启用' : '未启用'}</span>
            </button>

            <div className="schedule-section-label">每天执行时间</div>
            <div className="schedule-list">
              {times.length === 0
                ? <div className="schedule-empty">还没有时间点，点下方“添加时间”新增</div>
                : times.map((time, index) => (
                  <div className="schedule-row" key={index}>
                    <input
                      type="time"
                      value={time}
                      step={60}
                      aria-label={`第 ${index + 1} 个执行时间`}
                      onChange={(event) => updateTime(index, event.target.value)}
                    />
                    <button
                      type="button"
                      className="icon-button subtle"
                      aria-label={`删除第 ${index + 1} 个执行时间`}
                      title="删除该时间点"
                      onClick={() => setTimes((previous) => previous.filter((_, i) => i !== index))}
                    >
                      <Trash2 size={ICON.xs} />
                    </button>
                  </div>
                ))}
            </div>
            <Button
              tone="ghost"
              size="sm"
              icon={<Plus size={ICON.xs} />}
              disabled={saving || times.length >= MAX_TIMES}
              onClick={() => setTimes((previous) => [...previous, ''])}
            >
              添加时间
            </Button>

            <div className="schedule-note">
              <div>
                时间按系统时区执行：{formatTimezoneLabel(status.timezone, status.utc_offset)}
              </div>
              {status.enabled && status.next_run_at && !dirty && (
                <div>下次自动刷新：{formatNextRun(status.next_run_at, status.timezone, status.now)}</div>
              )}
              {dirty && enabled && draft.length > 0 && (
                <div>保存后按新时间执行。</div>
              )}
              {enabled && draft.length === 0 && (
                <div className="schedule-error">启用定时刷新至少需要一个时间点。</div>
              )}
            </div>
          </>
        )}

        <div className="modal-actions">
          <Button tone="ghost" size="sm" onClick={onClose}>取消</Button>
          <Button
            tone="primary"
            size="sm"
            loading={saving}
            disabled={!canSave}
            onClick={submit}
          >
            保存
          </Button>
        </div>
      </div>
    </div>
  )
}
