import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { ScheduleModal } from './ScheduleModal'
import { scheduledRefreshStatusMock } from '../testFixtures'
import type { ApiResponse, ScheduledRefreshStatus } from '../types/api'

function stubStatus(status: ScheduledRefreshStatus) {
  rs.stubGlobal('fetch', rs.fn(async () => ({
    ok: true,
    json: async () => ({
      success: true,
      message: 'ok',
      data: status,
    }) satisfies ApiResponse<ScheduledRefreshStatus>,
  })))
}

function renderModal(props = {}) {
  const onSave = rs.fn().mockResolvedValue(true)
  const onClose = rs.fn()
  const utils = render(
    <ScheduleModal open saving={false} onClose={onClose} onSave={onSave} {...props} />,
  )
  return { ...utils, onSave, onClose }
}

describe('ScheduleModal', () => {
  afterEach(() => {
    rs.unstubAllGlobals()
  })

  it('renders nothing when closed', () => {
    stubStatus(scheduledRefreshStatusMock())
    const { container } = renderModal({ open: false })
    expect(container).toBeEmptyDOMElement()
  })

  it('loads the saved schedule with timezone and next run', async () => {
    stubStatus(scheduledRefreshStatusMock({
      enabled: true,
      times: ['04:05', '20:00'],
      next_run_at: '2026-09-11T20:00:00+09:00',
    }))
    renderModal()

    const toggle = await screen.findByRole('switch', { name: '启用定时刷新' })
    expect(toggle).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByDisplayValue('04:05')).toBeInTheDocument()
    expect(screen.getByDisplayValue('20:00')).toBeInTheDocument()
    expect(screen.getByText(/Asia\/Tokyo（UTC\+09:00）/)).toBeInTheDocument()
    expect(screen.getByText(/下次自动刷新：今天 20:00/)).toBeInTheDocument()
  })

  it('saves the enabled draft sorted and deduplicated', async () => {
    stubStatus(scheduledRefreshStatusMock())
    const user = userEvent.setup()
    const { onSave, onClose } = renderModal()

    await user.click(await screen.findByRole('switch', { name: '启用定时刷新' }))
    await user.click(screen.getByRole('button', { name: /添加时间/ }))
    fireEvent.change(screen.getByLabelText('第 1 个执行时间'), { target: { value: '06:30' } })
    await user.click(screen.getByRole('button', { name: /添加时间/ }))
    fireEvent.change(screen.getByLabelText('第 2 个执行时间'), { target: { value: '04:05' } })
    await user.click(screen.getByRole('button', { name: /添加时间/ }))
    fireEvent.change(screen.getByLabelText('第 3 个执行时间'), { target: { value: '06:30' } })

    await user.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => expect(onSave).toHaveBeenCalledWith({
      enabled: true,
      times: ['04:05', '06:30'],
    }))
    expect(onClose).toHaveBeenCalled()
  })

  it('blocks saving when enabled without any time', async () => {
    stubStatus(scheduledRefreshStatusMock())
    const user = userEvent.setup()
    renderModal()

    await user.click(await screen.findByRole('switch', { name: '启用定时刷新' }))

    expect(screen.getByText('启用定时刷新至少需要一个时间点。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '保存' })).toBeDisabled()
  })

  it('removes a time row before saving', async () => {
    stubStatus(scheduledRefreshStatusMock({ enabled: true, times: ['04:05', '20:00'] }))
    const user = userEvent.setup()
    const { onSave } = renderModal()

    await screen.findByDisplayValue('04:05')
    await user.click(screen.getByRole('button', { name: '删除第 1 个执行时间' }))
    await user.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => expect(onSave).toHaveBeenCalledWith({
      enabled: true,
      times: ['20:00'],
    }))
  })

  it('shows a load error and keeps saving disabled', async () => {
    rs.stubGlobal('fetch', rs.fn(async () => {
      throw new Error('network down')
    }))
    renderModal()

    expect(await screen.findByText('加载定时刷新设置失败')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '保存' })).toBeDisabled()
  })
})
