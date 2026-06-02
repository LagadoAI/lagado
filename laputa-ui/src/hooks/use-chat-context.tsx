import { createContext, useContext, useState, useCallback, useRef, useEffect } from 'react'
import type { ReactNode } from 'react'

export interface Message {
  id: string
  role: 'user' | 'assistant'
  content: string
}

type ChatStatus = 'idle' | 'streaming' | 'submitted' | 'error'

interface ChatContextValue {
  messages: Message[]
  sendMessage: (payload: { text: string }) => void
  status: ChatStatus
  isPaused: boolean
  setIsPaused: (paused: boolean) => void
  idleOpacity: number
  setIdleOpacity: (opacity: number) => void
  chatBoxHidden: boolean
  setChatBoxHidden: (hidden: boolean) => void
}

const ChatContext = createContext<ChatContextValue | null>(null)

export function ChatProvider({ children }: { children: ReactNode }) {
  const [messages, setMessages] = useState<Message[]>([])
  const [status, setStatus] = useState<ChatStatus>('idle')
  const [isPaused, setIsPaused] = useState(false)
  const [chatBoxHidden, setChatBoxHidden] = useState(false)
  const [idleOpacity, setIdleOpacityState] = useState(() => {
    const saved = localStorage.getItem('ui-opacity')
    if (!saved) return 0.015
    const parsed = parseFloat(saved)
    return isNaN(parsed) ? 0.015 : parsed
  })

  const isMountedRef = useRef(true)
  useEffect(() => {
    return () => { isMountedRef.current = false }
  }, [])

  const setIdleOpacity = useCallback((opacity: number) => {
    setIdleOpacityState(opacity)
    localStorage.setItem('ui-opacity', opacity.toString())
  }, [])

  const sendMessage = useCallback(({ text }: { text: string }) => {
    if (!text.trim() || isPaused) return
    const userMsg: Message = { id: crypto.randomUUID(), role: 'user', content: text }
    setMessages(prev => [...prev, userMsg])
    setStatus('submitted')

    // TODO Phase 10: replace with useAgentSocket send over WebSocket :9090
    // ws.send(JSON.stringify({ type: 'chat', text }))
    // listen for { type: 'chat_response', content } and append to messages
    setTimeout(() => {
      if (!isMountedRef.current) return
      setStatus('streaming')
      const agentMsg: Message = {
        id: crypto.randomUUID(),
        role: 'assistant',
        content: 'Backend not connected yet — wire to useAgentSocket in Phase 10.',
      }
      setMessages(prev => [...prev, agentMsg])
      // Separate tick so 'streaming' state renders before flipping to 'idle'
      setTimeout(() => {
        if (!isMountedRef.current) return
        setStatus('idle')
      }, 100)
    }, 800)
  }, [isPaused])

  return (
    <ChatContext.Provider value={{
      messages, sendMessage, status,
      isPaused, setIsPaused,
      idleOpacity, setIdleOpacity,
      chatBoxHidden, setChatBoxHidden,
    }}>
      {children}
    </ChatContext.Provider>
  )
}

export function useChatContext() {
  const context = useContext(ChatContext)
  if (!context) throw new Error('useChatContext must be used within ChatProvider')
  return context
}
