import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import { ChatBox } from '@/components/ui/chat-box'
import { SidePane } from '@/components/ui/side-pane'
import { useChatContext } from '@/hooks/use-chat-context'
import { ChevronLeft } from 'lucide-react'

type CaptureSource = 'vm' | 'host'

export default function ImmersiveDefault() {
  const navigate = useNavigate()
  const { pendingPermission, approve, deny } = useChatContext()
  const [frameSrc, setFrameSrc] = useState<string>('')
  const [bootError, setBootError] = useState<string | null>(null)
  const captureNext = useRef<(() => void) | null>(null)

  // Source toggle — persisted
  const [captureSource, setCaptureSource] = useState<CaptureSource>(() =>
    (localStorage.getItem('immersive-source') as CaptureSource | null) ?? 'vm'
  )
  const captureSourceRef = useRef<CaptureSource>(captureSource)
  useEffect(() => {
    captureSourceRef.current = captureSource
    localStorage.setItem('immersive-source', captureSource)
  }, [captureSource])

  // Draggable ← Chat button position (Y only, left side)
  const [chatBtnY, setChatBtnY] = useState(() => {
    const saved = localStorage.getItem('chat-btn-y')
    const n = saved ? parseFloat(saved) : NaN
    return isNaN(n) ? 16 : n
  })
  const [isDraggingChat, setIsDraggingChat] = useState(false)
  const chatDragRef = useRef<{ mouseY: number; posY: number } | null>(null)

  const onChatMouseDown = (e: React.MouseEvent) => {
    e.preventDefault()
    setIsDraggingChat(true)
    chatDragRef.current = { mouseY: e.clientY, posY: chatBtnY }
  }

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!isDraggingChat || !chatDragRef.current) return
      const newY = Math.max(8, Math.min(
        window.innerHeight - 48,
        chatDragRef.current.posY + (e.clientY - chatDragRef.current.mouseY)
      ))
      setChatBtnY(newY)
    }
    const onUp = () => {
      if (isDraggingChat) {
        setIsDraggingChat(false)
        localStorage.setItem('chat-btn-y', chatBtnY.toString())
        chatDragRef.current = null
      }
    }
    if (isDraggingChat) {
      document.addEventListener('mousemove', onMove)
      document.addEventListener('mouseup', onUp)
    }
    return () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
    }
  }, [isDraggingChat, chatBtnY])

  // Capture loop — restarts when source changes
  useEffect(() => {
    let alive = true
    setFrameSrc('')
    setBootError(null)

    const doCapture = async () => {
      if (!alive) return
      try {
        const result = await invoke<string>('capture_frame', { source: captureSourceRef.current })
        if (!alive) return
        if (result === 'unchanged') {
          doCapture()
        } else {
          setFrameSrc(result)
        }
      } catch {
        if (!alive) return
        setTimeout(() => { if (alive) doCapture() }, 1500)
      }
    }

    captureNext.current = doCapture

    const start = async () => {
      if (captureSourceRef.current === 'vm') {
        try {
          await invoke('vm_boot')
        } catch (e: any) {
          const msg = e?.toString() ?? ''
          if (!msg.toLowerCase().includes('already running')) {
            if (!alive) return
            setBootError(msg)
            return
          }
        }
      }
      if (alive) doCapture()
    }

    start()
    return () => { alive = false; captureNext.current = null }
  }, [captureSource])

  return (
    <div className="h-screen bg-black overflow-hidden relative">

      {/* Live desktop feed */}
      {frameSrc ? (
        <img
          src={frameSrc}
          alt="Desktop"
          className="absolute inset-0 w-full h-full object-fill pointer-events-none"
          draggable={false}
          onLoad={() => captureNext.current?.()}
        />
      ) : (
        <div className="absolute inset-0 flex flex-col items-center justify-center bg-black gap-3">
          {bootError ? (
            <>
              <p className="text-white/30 text-sm">{bootError}</p>
              <button
                onClick={() => navigate('/vm')}
                className="px-4 py-2 text-xs text-lagado-blue border border-lagado-blue/30 rounded-lg bg-lagado-blue/10 hover:bg-lagado-blue/20 transition-colors"
              >
                Open VM Manager
              </button>
            </>
          ) : (
            <>
              <div className="w-2 h-2 rounded-full bg-white/20 animate-pulse" />
              <p className="text-white/20 text-xs">
                {captureSource === 'vm' ? 'Starting VM…' : 'Capturing host desktop…'}
              </p>
            </>
          )}
        </div>
      )}

      {/* ← Chat — draggable on left edge */}
      <button
        style={{ top: `${chatBtnY}px` }}
        onMouseDown={onChatMouseDown}
        onClick={() => { if (!isDraggingChat && !chatDragRef.current) navigate('/chat') }}
        className="fixed left-4 z-50 flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs text-white/40 hover:text-white/80 bg-black/30 hover:bg-black/50 backdrop-blur-sm border border-white/10 hover:border-white/20 transition-colors cursor-grab active:cursor-grabbing select-none"
      >
        <ChevronLeft className="w-3 h-3" />
        Chat
      </button>

      {/* Source toggle — draggable parity handled by its own position */}
      <button
        onClick={() => setCaptureSource(s => s === 'vm' ? 'host' : 'vm')}
        style={{ top: `${chatBtnY}px` }}
        className="fixed left-20 z-50 flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs bg-black/30 hover:bg-black/50 backdrop-blur-sm border border-white/10 hover:border-white/20 transition-colors select-none"
      >
        <span className={`w-1.5 h-1.5 rounded-full ${captureSource === 'vm' ? 'bg-lagado-purple' : 'bg-lagado-blue'}`} />
        <span className="text-white/40 hover:text-white/80">
          {captureSource === 'vm' ? 'VM' : 'Host'}
        </span>
      </button>

      {/* Floating ChatBox */}
      <ChatBox className="fixed bottom-8 left-1/2 -translate-x-1/2 z-40" />

      {/* SidePane — HITL approval only */}
      <SidePane>
        {pendingPermission && (
          <div className="bg-white/5 border border-yellow-500/40 rounded-xl p-4 shadow-[0_0_20px_rgba(245,158,11,0.1)]">
            <p className="text-xs text-yellow-400 font-semibold mb-1 tracking-wide">APPROVAL REQUIRED</p>
            <p className="text-sm text-white font-mono mb-1">{pendingPermission.action}</p>
            <p className="text-xs text-white/50 mb-3">{pendingPermission.reason ?? 'Confirm before executing'}</p>
            <div className="flex gap-2">
              <button
                onClick={() => approve(pendingPermission.id)}
                className="flex-1 py-1.5 bg-green-500/20 border border-green-500/40 text-green-400 rounded-lg text-xs font-semibold hover:bg-green-500/30 transition-all"
              >
                Approve
              </button>
              <button
                onClick={() => deny(pendingPermission.id)}
                className="flex-1 py-1.5 bg-red-500/10 border border-red-500/30 text-red-400 rounded-lg text-xs font-semibold hover:bg-red-500/20 transition-all"
              >
                Deny
              </button>
            </div>
          </div>
        )}
      </SidePane>
    </div>
  )
}
