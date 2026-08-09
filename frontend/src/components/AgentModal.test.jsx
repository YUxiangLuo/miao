import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AgentModal } from './AgentModal.jsx'

function jsonResponse(payload, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => payload,
  }
}

const providers = [
  { id: 'openai', label: 'OpenAI' },
  { id: 'anthropic', label: 'Anthropic' },
]

class FakeWebSocket {
  static CONNECTING = 0
  static OPEN = 1
  static CLOSING = 2
  static CLOSED = 3
  static instances = []

  constructor(url) {
    this.url = url
    this.readyState = FakeWebSocket.CONNECTING
    this.sent = []
    FakeWebSocket.instances.push(this)
  }

  open() {
    this.readyState = FakeWebSocket.OPEN
    this.onopen?.()
  }

  emit(payload) {
    this.onmessage?.({ data: JSON.stringify(payload) })
  }

  send(message) {
    this.sent.push(JSON.parse(message))
  }

  close() {
    this.readyState = FakeWebSocket.CLOSED
    this.onclose?.({ code: 1000 })
  }
}

describe('AgentModal', () => {
  beforeEach(() => {
    FakeWebSocket.instances = []
    vi.stubGlobal('WebSocket', FakeWebSocket)
    Element.prototype.scrollIntoView = vi.fn()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('configures an API-key provider without exposing the key input', async () => {
    const fetchMock = vi.fn(async (input, options = {}) => {
      if (input === '/api/agent/status') {
        return jsonResponse({
          success: true,
          data: {
            supported: true,
            configured: false,
            installed: false,
            providers,
            required_space_bytes: 268435456,
            available_space_bytes: 1073741824,
          },
        })
      }
      if (input === '/api/agent/config' && options.method === 'POST') {
        return jsonResponse({
          success: true,
          data: {
            supported: true,
            configured: true,
            installed: false,
            provider: 'openai',
            model: 'gpt-test',
            providers,
          },
        })
      }
      throw new Error(`Unexpected request: ${input}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    const user = userEvent.setup()
    render(<AgentModal open onClose={vi.fn()} />)

    const keyInput = await screen.findByLabelText('API Key')
    expect(screen.getByLabelText('Provider')).toHaveFocus()
    expect(keyInput).toHaveAttribute('type', 'password')
    await user.type(screen.getByLabelText('模型 ID（可选）'), 'gpt-test')
    await user.type(keyInput, 'sk-local-secret')
    await user.click(screen.getByRole('button', { name: '保存并启动助手' }))

    await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1))
    expect(fetchMock).toHaveBeenCalledWith('/api/agent/config', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({
        provider: 'openai',
        model: 'gpt-test',
        api_key: 'sk-local-secret',
      }),
    }))
    expect(screen.queryByDisplayValue('sk-local-secret')).not.toBeInTheDocument()
  })

  it('clears a rejected API key when the dialog closes', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input, options = {}) => {
      if (input === '/api/agent/config' && options.method === 'POST') {
        return jsonResponse({ success: false, message: 'Provider 配置失败' }, 400)
      }
      return jsonResponse({
        success: true,
        data: {
          supported: true,
          configured: false,
          installed: false,
          providers,
        },
      })
    }))

    const user = userEvent.setup()
    const { rerender } = render(<AgentModal open onClose={vi.fn()} />)
    const keyInput = await screen.findByLabelText('API Key')
    await user.type(keyInput, 'rejected-secret')
    await user.click(screen.getByRole('button', { name: '保存并启动助手' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Provider 配置失败')

    rerender(<AgentModal open={false} onClose={vi.fn()} />)
    rerender(<AgentModal open onClose={vi.fn()} />)
    expect(await screen.findByLabelText('API Key')).toHaveValue('')
  })

  it('translates the typed browser protocol into a streaming chat UI', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      success: true,
      data: {
        supported: true,
        configured: true,
        installed: true,
        provider: 'openai',
        model: 'gpt-test',
        providers,
      },
    })))

    const user = userEvent.setup()
    render(<AgentModal open onClose={vi.fn()} />)

    await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1))
    const socket = FakeWebSocket.instances[0]
    socket.open()
    socket.emit({ type: 'ready', provider: 'openai', model: 'gpt-test' })

    const composer = await screen.findByLabelText('发送给智能助手的消息')
    expect(composer).toHaveFocus()
    await user.type(composer, '如何检查订阅？')
    await user.click(screen.getByRole('button', { name: '发送消息' }))

    expect(socket.sent).toEqual([{ type: 'prompt', message: '如何检查订阅？' }])
    expect(screen.getByText('如何检查订阅？')).toBeInTheDocument()

    socket.emit({ type: 'working' })
    socket.emit({ type: 'text_delta', delta: '请先' })
    socket.emit({ type: 'text_delta', delta: '刷新订阅。' })
    socket.emit({ type: 'message_end', text: '请先刷新订阅。' })
    socket.emit({ type: 'settled' })

    expect(await screen.findByText('请先刷新订阅。')).toBeInTheDocument()

    await user.type(composer, '再试一次')
    await user.click(screen.getByRole('button', { name: '发送消息' }))
    socket.emit({ type: 'request_error', message: 'Provider 认证失败，请检查 API Key' })
    socket.emit({ type: 'settled' })

    expect(await screen.findByRole('alert')).toHaveTextContent('Provider 认证失败')
    expect(composer).toBeEnabled()
  })
})
