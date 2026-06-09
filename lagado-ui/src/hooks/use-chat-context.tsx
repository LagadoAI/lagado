import { createContext, useContext, useState, useCallback, useRef, useEffect } from 'react'
import type { ReactNode } from 'react'
import { useTauriAgent } from './useTauriAgent'
import type { PermissionRequest, ConnState } from './useTauriAgent'

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
  pendingPermission: PermissionRequest | null
  approve: (id: string) => void
  deny: (id: string) => void
  connState: ConnState
}

const ChatContext = createContext<ChatContextValue | null>(null)

export function ChatProvider({ children }: { children: ReactNode }) {
  const [messages, setMessages] = useState<Message[]>([])
  const [status, setStatus] = useState<ChatStatus>('idle')
  const [isPaused, setIsPaused] = useState(false)
  const [chatBoxHidden, setChatBoxHidden] = useState(false)
  const [pendingPermission, setPendingPermission] = useState<PermissionRequest | null>(null)
  const [idleOpacity, setIdleOpacityState] = useState(() => {
    const saved = localStorage.getItem('ui-opacity')
    if (!saved) return 0.015
    const parsed = parseFloat(saved)
    return isNaN(parsed) ? 0.015 : parsed
  })

  const isMountedRef = useRef(true)
  useEffect(() => {
    isMountedRef.current = true
    return () => { isMountedRef.current = false }
  }, [])

  const setIdleOpacity = useCallback((opacity: number) => {
    setIdleOpacityState(opacity)
    localStorage.setItem('ui-opacity', opacity.toString())
  }, [])

  const socket = useTauriAgent({
    onPermissionRequest: useCallback((req: PermissionRequest) => {
      if (!isMountedRef.current) return
      setPendingPermission(req)
    }, []),

    onMessage: useCallback((text: string) => {
      if (!isMountedRef.current) return
      const msg: Message = { id: crypto.randomUUID(), role: 'assistant', content: text }
      setMessages(prev => [...prev, msg])
      setStatus('idle')
    }, []),

    onStatus: useCallback((s: { state: string; detail: string }) => {
      if (!isMountedRef.current) return
      const msg: Message = {
        id: crypto.randomUUID(),
        role: 'assistant',
        content: s.detail ? `[${s.state}] ${s.detail}` : `[${s.state}]`,
      }
      setMessages(prev => [...prev, msg])
      if (s.state === 'goal_done' || s.state === 'goal_aborted') {
        setStatus('idle')
      }
    }, []),
  })

  const sendMessage = useCallback(({ text }: { text: string }) => {
    if (!text.trim() || isPaused) return
    const userMsg: Message = { id: crypto.randomUUID(), role: 'user', content: text }
    setMessages(prev => [...prev, userMsg])
    setStatus('submitted')
    socket.sendGoal(text)
  }, [isPaused, socket])

  const approve = useCallback((id: string) => {
    socket.sendApproval(id, true)
    setPendingPermission(null)
  }, [socket])

  const deny = useCallback((id: string) => {
    socket.sendApproval(id, false)
    setPendingPermission(null)
  }, [socket])

  return (
    <ChatContext.Provider value={{
      messages, sendMessage, status,
      isPaused, setIsPaused,
      idleOpacity, setIdleOpacity,
      chatBoxHidden, setChatBoxHidden,
      pendingPermission, approve, deny,
      connState: socket.connState,
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
