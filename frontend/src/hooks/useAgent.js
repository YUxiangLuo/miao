import { useCallback, useEffect, useRef, useState } from 'react'

const INITIAL_PHASE = { name: 'idle', message: '' }

function websocketUrl() {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${protocol}//${window.location.host}/api/agent/ws`
}

export function useAgent(open) {
  const socketRef = useRef(null)
  const messageIdRef = useRef(0)
  const serverErrorRef = useRef(false)
  const generationRef = useRef(0)
  const [status, setStatus] = useState(null)
  const [loading, setLoading] = useState(false)
  const [phase, setPhase] = useState(INITIAL_PHASE)
  const [messages, setMessages] = useState([])
  const [streaming, setStreaming] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [activeModel, setActiveModel] = useState(null)
  const [showSetup, setShowSetup] = useState(false)

  const disconnect = useCallback(() => {
    const socket = socketRef.current
    socketRef.current = null
    if (socket && socket.readyState < WebSocket.CLOSING) {
      try {
        socket.close(1000, 'dialog closed')
      } catch {
        // A browser may reject close() while the handshake is still pending.
      }
    }
    setStreaming(false)
  }, [])

  const connect = useCallback(() => {
    const existing = socketRef.current
    if (existing && existing.readyState < WebSocket.CLOSING) return

    serverErrorRef.current = false
    setError('')
    setNotice('')
    setPhase({ name: 'connecting', message: '正在连接助手…' })
    const socket = new WebSocket(websocketUrl())
    socketRef.current = socket

    socket.onmessage = (event) => {
      if (socketRef.current !== socket) return
      let payload
      try {
        payload = JSON.parse(event.data)
      } catch {
        serverErrorRef.current = true
        setError('助手返回了无效响应')
        return
      }

      switch (payload.type) {
        case 'phase':
          setPhase({ name: payload.phase, message: payload.message || '正在准备助手…' })
          break
        case 'ready':
          setActiveModel({ provider: payload.provider, model: payload.model })
          setPhase({ name: 'ready', message: 'Pi Agent 已就绪' })
          break
        case 'working':
          setStreaming(true)
          setPhase({ name: 'working', message: '正在思考…' })
          break
        case 'text_delta':
          setMessages((current) => {
            const next = [...current]
            const last = next[next.length - 1]
            if (!last || last.role !== 'assistant') {
              next.push({ id: ++messageIdRef.current, role: 'assistant', text: payload.delta || '', pending: true })
            } else {
              next[next.length - 1] = { ...last, text: last.text + (payload.delta || '') }
            }
            return next
          })
          break
        case 'message_end':
          setMessages((current) => {
            const next = [...current]
            const last = next[next.length - 1]
            if (!last || last.role !== 'assistant') {
              next.push({ id: ++messageIdRef.current, role: 'assistant', text: payload.text || '', pending: false })
            } else {
              next[next.length - 1] = { ...last, text: payload.text || last.text, pending: false }
            }
            return next
          })
          break
        case 'settled':
          setStreaming(false)
          setNotice('')
          setPhase({ name: 'ready', message: 'Pi Agent 已就绪' })
          setMessages((current) => current
            .map((message, index) => (
              index === current.length - 1 && message.role === 'assistant'
                ? { ...message, pending: false }
                : message
            ))
            .filter((message) => message.role !== 'assistant' || message.text))
          break
        case 'notice':
          setNotice(payload.message || '')
          break
        case 'request_error':
          setStreaming(false)
          setNotice('')
          setError(payload.message || 'Provider 请求失败')
          setPhase({ name: 'ready', message: 'Pi Agent 已就绪' })
          setMessages((current) => current.filter((message) => (
            message.role !== 'assistant' || message.text || !message.pending
          )))
          break
        case 'error':
          serverErrorRef.current = true
          setStreaming(false)
          setError(payload.message || '助手运行失败')
          setPhase({ name: 'error', message: '助手运行失败' })
          break
        default:
          break
      }
    }

    socket.onerror = () => {
      if (socketRef.current === socket && !serverErrorRef.current) setError('无法连接 Pi Agent')
    }
    socket.onclose = () => {
      if (socketRef.current !== socket) return
      socketRef.current = null
      setStreaming(false)
      setPhase((current) => current.name === 'error' ? current : { name: 'closed', message: '助手已关闭' })
    }
  }, [])

  const loadStatus = useCallback(async (generation) => {
    setLoading(true)
    setError('')
    try {
      const response = await fetch('/api/agent/status', { cache: 'no-store' })
      const payload = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || '无法读取助手状态')
      if (generation !== generationRef.current) return null
      setStatus(payload.data)
      const needsSetup = !payload.data.configured
      setShowSetup(needsSetup)
      if (payload.data.supported && payload.data.configured) connect()
      return payload.data
    } catch (requestError) {
      if (generation === generationRef.current) setError(requestError.message)
      return null
    } finally {
      if (generation === generationRef.current) setLoading(false)
    }
  }, [connect])

  const configure = useCallback(async ({ provider, model, apiKey }) => {
    const generation = generationRef.current
    setLoading(true)
    setError('')
    try {
      const response = await fetch('/api/agent/config', {
        method: 'POST',
        cache: 'no-store',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ provider, model: model.trim() || null, api_key: apiKey }),
      })
      const payload = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || 'Provider 配置失败')
      if (generation !== generationRef.current) return false
      setStatus(payload.data)
      setShowSetup(false)
      connect()
      return true
    } catch (requestError) {
      if (generation === generationRef.current) setError(requestError.message)
      return false
    } finally {
      if (generation === generationRef.current) setLoading(false)
    }
  }, [connect])

  const sendPrompt = useCallback((text) => {
    const message = text.trim()
    const socket = socketRef.current
    if (!message || !socket || socket.readyState !== WebSocket.OPEN || phase.name !== 'ready' || streaming) {
      return false
    }

    setError('')
    setNotice('')
    setMessages((current) => [
      ...current,
      { id: ++messageIdRef.current, role: 'user', text: message },
      { id: ++messageIdRef.current, role: 'assistant', text: '', pending: true },
    ])
    setStreaming(true)
    setPhase({ name: 'working', message: '正在思考…' })
    socket.send(JSON.stringify({ type: 'prompt', message }))
    return true
  }, [phase.name, streaming])

  const abort = useCallback(() => {
    const socket = socketRef.current
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'abort' }))
    }
  }, [])

  const reconfigure = useCallback(() => {
    disconnect()
    setShowSetup(true)
    setError('')
    setPhase(INITIAL_PHASE)
    setActiveModel(null)
  }, [disconnect])

  useEffect(() => {
    if (!open) {
      disconnect()
      return undefined
    }

    const generation = ++generationRef.current
    setStatus(null)
    setShowSetup(false)
    setMessages([])
    setActiveModel(null)
    setPhase(INITIAL_PHASE)
    loadStatus(generation)
    return () => {
      generationRef.current += 1
      disconnect()
    }
  }, [open, disconnect, loadStatus])

  return {
    status,
    loading,
    phase,
    messages,
    streaming,
    error,
    notice,
    activeModel,
    showSetup,
    configure,
    sendPrompt,
    abort,
    reconfigure,
  }
}
