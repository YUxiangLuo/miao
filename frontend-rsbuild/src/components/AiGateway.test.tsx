import { StrictMode } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { AiGateway } from './AiGateway'
import type { AiSettingsView } from '../types/api'

afterEach(() => { rs.restoreAllMocks() })

function setup(rejectSave = false) {
  const settings: AiSettingsView = { revision: 2, enabled: false, port: 6162, proxy_port: 6163, outbound: 'proxy', has_access_key: true,
    providers: [{ id: 'fixture', name: 'OpenRouter', base_url: 'https://openrouter.ai/api/v1', has_key: true, responses: false, models: [{ id: 'alias', upstream: 'vendor/model' }] }] }
  const fetcher = rs.spyOn(globalThis, 'fetch').mockImplementation(async (url, init) => {
    if (String(url).endsWith('/status')) return Response.json({ enabled: false, ready: false, port: 6162, outbound: 'proxy', error: null, admin_token_path: '/fixture/admin-token.txt' })
    if (String(url).endsWith('/access-key')) return Response.json({ key: 'synthetic-new-key', revision: 3 })
    if (init?.method === 'POST' && rejectSave) return Response.json({ error: 'AI 配置已变化，请重新加载后保存' }, { status: 400 })
    return Response.json(settings)
  })
  render(<StrictMode><AiGateway nodeNames={['node-a']} /></StrictMode>)
  return fetcher
}

async function unlock(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText('管理员令牌'), 'synthetic-admin')
  await user.click(screen.getByRole('button', { name: '读取配置' }))
  await screen.findByLabelText('Base URL')
}

describe('AI gateway configuration', () => {
  it('keeps model input focus, preserves stored keys, and uses uncached authenticated requests', async () => {
    const fetcher = setup()
    const user = userEvent.setup()
    await unlock(user)
    await user.type(screen.getByLabelText('模型别名'), '-edited')
    expect(screen.getByLabelText('模型别名')).toHaveValue('alias-edited')
    expect(screen.getByLabelText('API 密钥')).toHaveValue('')
    await user.click(screen.getByRole('button', { name: '保存配置' }))
    await waitFor(() => expect(fetcher.mock.calls.some(([, init]) => init?.method === 'POST')).toBe(true))
    const [, init] = fetcher.mock.calls.find(([, init]) => init?.method === 'POST')!
    expect(init?.cache).toBe('no-store')
    expect(init?.headers).toMatchObject({ Authorization: 'Bearer synthetic-admin' })
    expect(JSON.parse(String(init?.body)).providers[0]).toMatchObject({ api_key: null, models: [{ id: 'alias-edited', upstream: 'vendor/model' }] })
  })

  it('keeps rejected drafts and offers reload; locking clears administrator credentials', async () => {
    setup(true)
    const user = userEvent.setup()
    await unlock(user)
    await user.type(screen.getByLabelText('名称'), ' draft')
    await user.click(screen.getByRole('button', { name: '保存配置' }))
    await screen.findByRole('button', { name: '放弃草稿并重新加载' })
    expect(screen.getByLabelText('名称')).toHaveValue('OpenRouter draft')
    await user.click(screen.getByRole('button', { name: '锁定配置' }))
    expect(screen.getByLabelText('管理员令牌')).toHaveValue('')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: '切换到代理' })).toHaveAttribute('href', '#proxy')
  })

  it('rotation preserves draft and advances its revision without fetching over concurrent edits', async () => {
    const fetcher = setup()
    rs.spyOn(window, 'confirm').mockReturnValue(true)
    const user = userEvent.setup()
    await unlock(user)
    await user.type(screen.getByLabelText('名称'), ' draft')
    await user.click(screen.getByRole('button', { name: '轮换' }))
    await screen.findByText('synthetic-new-key')
    expect(screen.getByLabelText('名称')).toHaveValue('OpenRouter draft')
    await user.click(screen.getByRole('button', { name: '保存配置' }))
    await waitFor(() => expect(fetcher.mock.calls.some(([url, init]) => String(url).endsWith('/settings') && init?.method === 'POST')).toBe(true))
    const [, init] = fetcher.mock.calls.find(([url, init]) => String(url).endsWith('/settings') && init?.method === 'POST')!
    expect(JSON.parse(String(init?.body)).revision).toBe(3)
  })
})
