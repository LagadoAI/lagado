import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useChatContext } from '@/hooks/use-chat-context'
import { ChevronLeft } from 'lucide-react'

// True when rendered inside the separate Agent OS window (vs the /agent route in the main window).
const IS_AGENT_WINDOW = new URLSearchParams(window.location.search).get('view') === 'agent'

// The AGENT work surface: the bare VM the agent operates. No chat, no toggles, no chrome — you
// watch the agent work the (sovereign, sandboxed) computer here; you DIRECT it from the control
// surface. VM only — host capture is removed by design (the sandbox boundary is the product).
export default function ImmersiveDefault() {
  const navigate = useNavigate()
  const { pendingPermission, approve, deny } = useChatContext()
  const [frameSrc, setFrameSrc] = useState<string>('')
  const [bootError, setBootError] = useState<string | null>(null)
  const captureNext = useRef<(() => void) | null>(null)

  // Capture loop — VM only.
  useEffect(() => {
    let alive = true
    setFrameSrc('')
    setBootError(null)

    const doCapture = async () => {
      if (!alive) return
      try {
        const result = await invoke<string>('capture_frame', { source: 'vm' })
        if (!alive) return
        if (result === 'unchanged') doCapture()
        else setFrameSrc(result)
      } catch {
        if (!alive) return
        setTimeout(() => { if (alive) doCapture() }, 1500)
      }
    }
    captureNext.current = doCapture

    const start = async () => {
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
      if (alive) doCapture()
    }

    start()
    return () => { alive = false; captureNext.current = null }
  }, [])

  return (
    <div className="h-screen bg-black overflow-hidden relative">
      {/* Live VM feed — the whole surface */}
      {frameSrc ? (
        <img
          src={frameSrc}
          alt="Agent VM"
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
              <p className="text-white/20 text-xs">Starting VM…</p>
            </>
          )}
        </div>
      )}

      {/* The only chrome: a minimal affordance back to control. In the separate Agent window it
          closes the window (control lives in the main window); on the /agent route it navigates. */}
      <button
        onClick={() => { if (IS_AGENT_WINDOW) getCurrentWindow().close(); else navigate('/chat') }}
        style={{ background: 'var(--glass-opaque)' }}
        className="fixed top-4 left-4 z-50 flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs text-white/40 hover:text-white/80 border border-white/10 hover:border-white/20 transition-colors select-none"
        aria-label="Back to control surface"
      >
        <ChevronLeft className="w-3 h-3" />
        {IS_AGENT_WINDOW ? 'Close' : 'Control'}
      </button>

      {/* HITL approval — must surface even on the bare VM (safety). Minimal bottom overlay. */}
      {pendingPermission && (
        <div
          style={{ background: 'var(--glass-opaque)' }}
          className="fixed bottom-6 left-1/2 -translate-x-1/2 z-50 w-[min(92vw,440px)] rounded-xl p-4 border border-yellow-500/40 shadow-[0_0_20px_rgba(245,158,11,0.15)]"
        >
          <p className="text-xs text-yellow-400 font-semibold mb-1 tracking-wide">APPROVAL REQUIRED</p>
          <p className="text-sm text-white font-mono mb-1 break-all">{pendingPermission.action}</p>
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
    </div>
  )
}
