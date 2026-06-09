import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { ChatBox } from '@/components/ui/chat-box'
import { SidePane } from '@/components/ui/side-pane'
import { useChatContext } from '@/hooks/use-chat-context'

export default function ImmersiveDefault() {
  const navigate = useNavigate()
  const { messages, isPaused, pendingPermission, approve, deny } = useChatContext()
  const [elapsed, setElapsed] = useState(0)
  const startRef = useRef(Date.now())
  const feedEndRef = useRef<HTMLDivElement>(null)

  // Elapsed timer
  useEffect(() => {
    const t = setInterval(() => setElapsed(Math.floor((Date.now() - startRef.current) / 1000)), 1000)
    return () => clearInterval(t)
  }, [])

  // Auto-scroll action feed
  useEffect(() => {
    feedEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, pendingPermission])

  // Derive current goal from last user message
  const currentGoal = [...messages].reverse().find(m => m.role === 'user')?.content ?? 'Waiting for instruction'

  // Format elapsed time
  const formatTime = (s: number) => `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`

  // Action feed entries — assistant messages only, shown as action cards
  const actionEntries = messages.filter(m => m.role === 'assistant')

  return (
    <div className="h-screen bg-lagado-bg overflow-hidden flex flex-col relative">

      {/* Ambient background glow — very subtle */}
      <div className="absolute inset-0 pointer-events-none">
        <div className="absolute top-1/3 left-1/2 -translate-x-1/2 w-[600px] h-[400px] rounded-full opacity-[0.04] bg-lagado-blue blur-[120px]" />
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 w-[400px] h-[300px] rounded-full opacity-[0.03] bg-lagado-purple blur-[100px]" />
      </div>

      {/* TOP HUD */}
      <div className="relative z-10 flex items-center gap-4 px-6 py-3 border-b border-lagado-border/40 bg-lagado-surface/40 backdrop-blur-md flex-shrink-0">
        <button
          onClick={() => navigate('/chat')}
          className="px-3 py-1.5 text-body-sm text-lagado-text-dim hover:text-lagado-text border border-lagado-border/60 rounded-lg hover:border-lagado-blue/60 transition-colors"
        >
          ← Chat
        </button>

        {/* Status pill */}
        <div className={`flex items-center gap-2 px-3 py-1 rounded-full border text-body-sm font-medium ${
          isPaused
            ? 'border-lagado-yellow/40 bg-lagado-yellow/10 text-lagado-yellow'
            : pendingPermission
            ? 'border-lagado-red/40 bg-lagado-red/10 text-lagado-red animate-pulse'
            : 'border-lagado-green/40 bg-lagado-green/10 text-lagado-green'
        }`}>
          <span className={`w-1.5 h-1.5 rounded-full ${
            isPaused ? 'bg-lagado-yellow' :
            pendingPermission ? 'bg-lagado-red' :
            'bg-lagado-green animate-pulse'
          }`} />
          {isPaused ? 'PAUSED' : pendingPermission ? 'AWAITING APPROVAL' : 'RUNNING'}
        </div>

        {/* Current goal */}
        <div className="flex-1 min-w-0">
          <p className="text-body-sm text-lagado-text-dim truncate">
            <span className="text-lagado-text-dim mr-2">Goal:</span>
            <span className="text-lagado-text">{currentGoal.slice(0, 80)}{currentGoal.length > 80 ? '…' : ''}</span>
          </p>
        </div>

        {/* Timer */}
        <span className="font-mono text-body-sm text-lagado-text-dim flex-shrink-0">{formatTime(elapsed)}</span>
      </div>

      {/* ACTION FEED */}
      <div className="relative z-10 flex-1 overflow-y-auto px-6 py-6 pb-40">
        <div className="max-w-2xl mx-auto space-y-3">
          {actionEntries.length === 0 && !pendingPermission && (
            <div className="flex flex-col items-center justify-center py-24 text-center">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-lagado-blue/20 to-lagado-purple/20 border border-lagado-border/40 flex items-center justify-center mb-4">
                <span className="text-lagado-text-dim text-lg">◆</span>
              </div>
              <p className="text-body text-lagado-text-dim">Agent standing by</p>
              <p className="text-body-sm text-lagado-text-dim/60 mt-1">Send a goal from the chat box below</p>
            </div>
          )}

          {actionEntries.map((entry, i) => (
            <div
              key={entry.id}
              className="bg-lagado-surface/50 backdrop-blur-md border border-lagado-border/50 rounded-xl p-4 flex items-start gap-3"
            >
              <div className="w-6 h-6 rounded-md bg-gradient-to-br from-lagado-blue/20 to-lagado-purple/20 border border-lagado-border/60 flex items-center justify-center flex-shrink-0 mt-0.5">
                <span className="text-lagado-purple text-caption font-bold">◆</span>
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-body-sm text-lagado-text leading-relaxed">{entry.content}</p>
              </div>
              <span className="text-caption text-lagado-green flex-shrink-0 mt-0.5">✓</span>
            </div>
          ))}

          {/* HITL APPROVAL CARD — prominent, center-stage */}
          {pendingPermission && (
            <div className="bg-lagado-surface/70 backdrop-blur-md border border-lagado-yellow/50 rounded-xl p-5 shadow-[0_0_30px_rgba(245,158,11,0.12)]">
              <div className="flex items-center gap-2 mb-3">
                <span className="w-2 h-2 rounded-full bg-lagado-yellow animate-pulse" />
                <span className="text-body-sm text-lagado-yellow font-semibold tracking-wide">APPROVAL REQUIRED</span>
              </div>
              <p className="text-body text-lagado-text-bright font-mono mb-1">{pendingPermission.action}</p>
              <p className="text-body-sm text-lagado-text-dim mb-5">{pendingPermission.reason ?? 'Action requires confirmation before executing'}</p>
              <div className="flex gap-3">
                <button
                  onClick={() => approve(pendingPermission.id)}
                  className="flex-1 py-2.5 bg-lagado-green/20 border border-lagado-green/50 text-lagado-green rounded-lg text-body-sm font-semibold hover:bg-lagado-green/30 hover:shadow-[0_0_12px_rgba(34,197,94,0.2)] transition-all"
                >
                  Approve
                </button>
                <button
                  onClick={() => deny(pendingPermission.id)}
                  className="flex-1 py-2.5 bg-lagado-red/10 border border-lagado-red/40 text-lagado-red rounded-lg text-body-sm font-semibold hover:bg-lagado-red/20 transition-all"
                >
                  Deny
                </button>
              </div>
            </div>
          )}

          <div ref={feedEndRef} />
        </div>
      </div>

      {/* ChatBox and SidePane — keep exactly as-is */}
      <ChatBox className="fixed bottom-8 left-1/2 -translate-x-1/2 z-40" />
      <SidePane />
    </div>
  )
}
