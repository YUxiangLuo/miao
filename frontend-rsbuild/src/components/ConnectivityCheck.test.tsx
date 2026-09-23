import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, expect, it, rs } from '@rstest/core'
import { ConnectivityCheck } from './ConnectivityCheck'
import { connectivityMock, statusMock } from '../testFixtures'
import type { ConnectivityResult } from '../types/api'

afterEach(() => { rs.unstubAllGlobals() })

function response(overrides: Partial<ConnectivityResult> = {}) {
  return new Response(JSON.stringify({ success: true, data: connectivityMock(overrides) }))
}

it.each([204, 403])('reports HTTP %s without claiming browser connectivity', async (status) => {
  const user = userEvent.setup()
  const fetch = rs.fn(async () => response({ http_status: status }))
  rs.stubGlobal('fetch', fetch)
  render(<ConnectivityCheck status={statusMock({ running: true })} backendUnreachable={false} />)
  expect(fetch).not.toHaveBeenCalled()
  await user.click(screen.getByRole('button', { name: '连接检查' }))
  await user.clear(screen.getByRole('textbox', { name: '目标网址' }))
  await user.type(screen.getByRole('textbox', { name: '目标网址' }), 'https://example.com/')
  await user.click(screen.getByRole('button', { name: '开始检查' }))
  const report = await screen.findByRole('textbox', { name: '检查报告' }) as HTMLTextAreaElement
  expect(report.value).toContain(`HTTP ${status}`)
  expect(report.value).toContain('耗时：73 ms')
  expect(report.value).toContain('目标：https://example.com/')
  expect(report.value).toContain('不是指定节点或浏览器测试')
  if (status === 403) expect(report.value).toContain('访问未成功')
  expect(fetch).toHaveBeenCalledWith('/api/connectivity', expect.objectContaining({ method: 'POST', body: JSON.stringify({ url: 'https://example.com/' }) }))
  await user.click(screen.getByRole('button', { name: '复制检查报告' }))
  expect(await navigator.clipboard.readText()).toBe(report.value)
})

it('retains a pending check across closing and preserves the original service snapshot', async () => {
  const user = userEvent.setup()
  let resolve!: (value: Response) => void
  const fetch = rs.fn(() => new Promise<Response>(done => { resolve = done }))
  rs.stubGlobal('fetch', fetch)
  const view = render(<ConnectivityCheck status={statusMock({ running: true })} backendUnreachable={false} />)
  await user.click(screen.getByRole('button', { name: '连接检查' }))
  await user.click(screen.getByRole('button', { name: '开始检查' }))
  expect(screen.getByRole('button', { name: '开始检查' })).toBeDisabled()
  await user.click(screen.getByRole('button', { name: '关闭连接检查' }))
  view.rerender(<ConnectivityCheck status={statusMock()} backendUnreachable={true} />)
  await act(async () => { resolve(response({ success: false, http_status: null, latency_ms: null, error_kind: 'timeout', error: 'operation timed out' })) })
  await user.click(screen.getByRole('button', { name: '连接检查' }))
  const report = screen.getByRole('textbox', { name: '检查报告' }) as HTMLTextAreaElement
  expect(report.value).toContain('检查开始时：代理服务已就绪')
  expect(report.value).toContain('请求超时：5 秒内未完成')
  expect(screen.getAllByText(/后端不可达，服务状态未知/).length).toBeGreaterThan(0)
  expect(fetch).toHaveBeenCalledTimes(1)
})

it('rejects unsupported URLs and distinguishes backend failure from target failure', async () => {
  const user = userEvent.setup()
  const fetch = rs.fn().mockRejectedValue(new Error('backend offline'))
  rs.stubGlobal('fetch', fetch)
  render(<ConnectivityCheck status={statusMock()} backendUnreachable={true} />)
  await user.click(screen.getByRole('button', { name: '连接检查' }))
  const input = screen.getByRole('textbox', { name: '目标网址' })
  await user.clear(input)
  await user.type(input, 'file:///tmp/a')
  await user.click(screen.getByRole('button', { name: '开始检查' }))
  expect(screen.getByRole('alert')).toHaveTextContent('HTTP 或 HTTPS')
  expect(fetch).not.toHaveBeenCalled()
  await user.clear(input)
  await user.type(input, 'https://example.com/')
  await user.click(screen.getByRole('button', { name: '开始检查' }))
  expect(await screen.findByText(/未取得检查结果，目标是否可达未知/, { selector: 'p' })).toBeInTheDocument()
  const report = screen.getByRole('textbox', { name: '检查报告' }) as HTMLTextAreaElement
  expect(report.value).toContain('backend offline')
  const copy = rs.spyOn(navigator.clipboard, 'writeText').mockRejectedValueOnce(new Error('denied'))
  try {
    await user.click(screen.getByRole('button', { name: '复制检查报告' }))
    expect(report).toHaveFocus()
    expect(report.selectionStart).toBe(0)
    expect(report.selectionEnd).toBe(report.value.length)
    expect(screen.getByText('无法自动复制，已选中报告，请手动复制。')).toBeInTheDocument()
  } finally { copy.mockRestore() }
})
