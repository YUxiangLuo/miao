import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { AiAnalytics } from './AiAnalytics'
import type { AiAnalytics as Data } from '../types/api'

afterEach(() => {
  rs.restoreAllMocks()
})
const data: Data = {
  since: 1000,
  until: 2000,
  dropped_records: 0,
  collection_error: null,
  next_before: 41,
  totals: {
    provider: '',
    model: '',
    requests: 3,
    succeeded: 1,
    failed: 1,
    cancelled: 1,
    usage_known: 0,
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    avg_duration_ms: 1350,
    p95_duration_ms: 2450,
  },
  groups: [],
  records: [
    {
      id: 42,
      timestamp: 1500,
      provider: 'fixture',
      model: 'alias',
      upstream_model: 'upstream',
      endpoint: 'chat/completions',
      streaming: true,
      outcome: 'cancelled',
      http_status: 200,
      duration_ms: 2000,
      first_byte_ms: 120,
      input_tokens: 13,
      output_tokens: 2,
      total_tokens: 15,
      usage_complete: false,
    },
  ],
}

describe('AI statistics', () => {
  it('uses private uncached queries and shows missing usage as unknown, not measured zero', async () => {
    const fetcher = rs.spyOn(globalThis, 'fetch').mockResolvedValue(Response.json(data))
    render(<AiAnalytics token="test-admin" view="overview" />)
    expect(await screen.findByText('未知')).toBeInTheDocument()
    expect(screen.getByText('完整用量 0 / 3 次请求')).toBeInTheDocument()
    expect(screen.getByText('成功 1 · 失败 1 · 取消 1')).toBeInTheDocument()
    expect(fetcher.mock.calls[0][1]).toMatchObject({
      cache: 'no-store',
      headers: { Authorization: 'Bearer test-admin' },
    })
  })

  it('retains incomplete usage, paginates, resets cursor on filter, and clears data on error', async () => {
    const fetcher = rs.spyOn(globalThis, 'fetch').mockImplementation(async () => Response.json(data))
    const user = userEvent.setup()
    render(<AiAnalytics token="test-admin" view="requests" />)
    expect(await screen.findByText('13 / 2 / 15')).toBeInTheDocument()
    expect(screen.getByText('不完整，不计入汇总')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '更早记录' }))
    await waitFor(() => expect(String(fetcher.mock.lastCall?.[0])).toContain('before=41'))
    await user.selectOptions(screen.getByLabelText('时间范围'), '7')
    await waitFor(() => expect(String(fetcher.mock.lastCall?.[0])).toContain('days=7'))
    expect(String(fetcher.mock.lastCall?.[0])).not.toContain('before=')
    fetcher.mockResolvedValue(Response.json({}, { status: 503 }))
    await user.click(screen.getByRole('button', { name: '刷新统计' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('统计读取失败')
    expect(screen.queryByText('13 / 2 / 15')).not.toBeInTheDocument()
  })
})
