import { useNavigate } from 'react-router-dom'
import { ChatBox } from '@/components/ui/chat-box'
import { SidePane } from '@/components/ui/side-pane'
import { useChatContext } from '@/hooks/use-chat-context'

export default function ImmersiveDefault() {
  const navigate = useNavigate()
  const { messages, isPaused, pendingPermission, approve, deny } = useChatContext()

  // Action entries for the SidePane feed
  const actionEntries = messages.filter(m => m.role === 'assistant')

  return (
    <div className="h-screen bg-black overflow-hidden relative">

      {/*
        Phase 1: black canvas representing the desktop.
        Phase 2: replace this div with a live screen capture feed
        from /dev/shm/lagado_frame.png at 20Hz via the capture.rs backend.
      */}
      <div className="absolute inset-0 bg-black" />

      {/* Floating ChatBox — draggable, positioned by user */}
      <ChatBox className="fixed bottom-8 left-1/2 -translate-x-1/2 z-40" />

      {/* SidePane — action feed + status passed as children */}
      <SidePane>
        {/* Status */}
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

        {/* HITL approval card */}
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

        {/* Action feed */}
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
