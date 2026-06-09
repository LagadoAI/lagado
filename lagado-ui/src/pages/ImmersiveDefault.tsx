import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import { ChatBox } from '@/components/ui/chat-box'
import { SidePane } from '@/components/ui/side-pane'
import { useChatContext } from '@/hooks/use-chat-context'

export default function ImmersiveDefault() {
  const navigate = useNavigate()
  const { messages, isPaused, pendingPermission, approve, deny } = useChatContext()
  const [frameSrc, setFrameSrc] = useState<string>('')
  const [captureError, setCaptureError] = useState(false)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  // Self-clocked capture loop — exactly one frame in flight at a time.
  // Next capture starts only after the current frame finishes loading (onLoad).
  // This bounds memory to one frame regardless of how slow capture gets.
  const captureNext = useRef<(() => void) | null>(null)

  useEffect(() => {
    let alive = true

    const doCapture = async () => {
      if (!alive) return
      try {
        const result = await invoke<string>('capture_frame')
        setCaptureError(false)
        if (result === 'unchanged') {
          // Screen didn't change — skip the src swap, trigger next capture immediately
          doCapture()
        } else {
          setFrameSrc(result)
          // Next capture triggered by onLoad on the <img> element
        }
      } catch {
        setCaptureError(true)
        // On error, retry after a pause
        setTimeout(() => { if (alive) doCapture() }, 500)
      }
    }

    captureNext.current = doCapture
    doCapture()

    return () => { alive = false; captureNext.current = null }
  }, [])

  const actionEntries = messages.filter(m => m.role === 'assistant')

  return (
    <div className="h-screen bg-black overflow-hidden relative">

      {/* Live desktop feed */}
      {frameSrc && !captureError ? (
        <img
          src={frameSrc}
          alt="Desktop"
          className="absolute inset-0 w-full h-full object-cover pointer-events-none"
          draggable={false}
          onLoad={() => captureNext.current?.()}
        />
      ) : (
        // Fallback while waiting for first frame or on capture error
        <div className="absolute inset-0 flex flex-col items-center justify-center bg-black">
          {captureError ? (
            <>
              <p className="text-white/20 text-sm mb-1">Screen capture unavailable</p>
              <p className="text-white/10 text-xs">Install grim (Wayland) or scrot (X11)</p>
            </>
          ) : (
            <div className="w-2 h-2 rounded-full bg-white/10 animate-pulse" />
          )}
        </div>
      )}

      {/* Floating ChatBox — draggable by user */}
      <ChatBox className="fixed bottom-8 left-1/2 -translate-x-1/2 z-40" />

      {/* SidePane — status, HITL approval, action feed */}
      <SidePane>
        <div className={`flex items-center gap-2 px-3 py-1.5 rounded-full border text-xs font-medium mb-3 w-fit ${
          isPaused
            ? 'border-yellow-500/40 bg-yellow-500/10 text-yellow-400'
            : pendingPermission
            ? 'border-red-500/40 bg-red-500/10 text-red-400 animate-pulse'
            : 'border-green-500/40 bg-green-500/10 text-green-400'
        }`}>
          <span className={`w-1.5 h-1.5 rounded-full ${
            isPaused ? 'bg-yellow-400' :
            pendingPermission ? 'bg-red-400' :
            'bg-green-400 animate-pulse'
          }`} />
          {isPaused ? 'Paused' : pendingPermission ? 'Awaiting approval' : 'Running'}
        </div>

        {pendingPermission && (
          <div className="bg-white/5 border border-yellow-500/40 rounded-xl p-4 mb-3 shadow-[0_0_20px_rgba(245,158,11,0.1)]">
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

        {actionEntries.length > 0 && (
          <div className="space-y-2">
            <p className="text-xs text-white/30 uppercase tracking-wider mb-2">Actions</p>
            {actionEntries.slice(-8).map((entry) => (
              <div key={entry.id} className="flex items-start gap-2 bg-white/5 rounded-lg px-3 py-2">
                <span className="text-purple-400 text-xs mt-0.5 flex-shrink-0">◆</span>
                <p className="text-xs text-white/60 leading-relaxed">{entry.content}</p>
              </div>
            ))}
          </div>
        )}

        {actionEntries.length === 0 && !pendingPermission && (
          <p className="text-xs text-white/20 text-center py-4">No actions yet</p>
        )}
      </SidePane>
    </div>
  )
}
