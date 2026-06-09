import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'

type VmStatus = { running: boolean; ssh_port?: number }

export default function VMManager() {
  const navigate = useNavigate()
  const [status, setStatus] = useState<VmStatus>({ running: false })
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const poll = async () => {
    try {
      const s = await invoke<VmStatus>('vm_status')
      setStatus(s)
    } catch {}
  }

  useEffect(() => {
    poll()
    const id = setInterval(poll, 3000)
    return () => clearInterval(id)
  }, [])

  const boot = async () => {
    setLoading(true)
    setError(null)
    try {
      await invoke('vm_boot')
      await poll()
    } catch (e: any) {
      setError(e?.toString() ?? 'Boot failed')
    } finally {
      setLoading(false)
    }
  }

  const stop = async () => {
    setLoading(true)
    setError(null)
    try {
      await invoke('vm_stop')
      setStatus({ running: false })
    } catch (e: any) {
      setError(e?.toString() ?? 'Stop failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      {/* Nav */}
      <div className="border-b border-lagado-border bg-lagado-surface px-4 py-3 flex items-center gap-3">
        <button
          onClick={() => navigate('/chat')}
          className="px-3 py-1.5 text-xs text-lagado-text-dim hover:text-lagado-text border border-lagado-border rounded-md hover:border-lagado-border-light transition-colors"
        >
          ← Chat
        </button>
        <span className="text-lagado-text-bright font-semibold">VM Manager</span>
      </div>

      <div className="flex-1 p-6 max-w-2xl mx-auto w-full space-y-4">

        {/* Status card */}
        <div className="bg-lagado-surface border border-lagado-border rounded-xl p-5">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lagado-text-bright font-semibold">Arch Linux — XFCE4</h2>
            <span className={`flex items-center gap-1.5 text-xs font-medium px-2.5 py-1 rounded-full border ${
              status.running
                ? 'border-green-500/40 bg-green-500/10 text-green-400'
                : 'border-lagado-border bg-lagado-surface-2 text-lagado-text-dim'
            }`}>
              <span className={`w-1.5 h-1.5 rounded-full ${status.running ? 'bg-green-400 animate-pulse' : 'bg-lagado-text-dim'}`} />
              {status.running ? 'Running' : 'Stopped'}
            </span>
          </div>

          <div className="grid grid-cols-2 gap-2 mb-4 text-xs">
            {[
              ['CPU', '4 vCPUs (KVM host)'],
              ['RAM', '4 GB'],
              ['Disk', 'Arch-Linux-x86_64-cloudimg.qcow2'],
              ['SSH', status.running ? `localhost:${status.ssh_port ?? 2222}` : '—'],
            ].map(([k, v]) => (
              <div key={k} className="bg-lagado-surface-2 border border-lagado-border rounded-lg px-3 py-2">
                <p className="text-lagado-text-dim mb-0.5">{k}</p>
                <p className="text-lagado-text font-mono truncate">{v}</p>
              </div>
            ))}
          </div>

          {error && (
            <div className="mb-3 px-3 py-2 bg-red-500/10 border border-red-500/30 rounded-lg text-xs text-red-400">
              {error}
            </div>
          )}

          <div className="flex gap-2">
            {!status.running ? (
              <button
                onClick={boot}
                disabled={loading}
                className="flex-1 py-2 bg-lagado-blue/20 border border-lagado-blue/40 text-lagado-blue rounded-lg text-sm font-semibold hover:bg-lagado-blue/30 disabled:opacity-50 disabled:cursor-not-allowed transition-all"
              >
                {loading ? 'Booting…' : 'Boot VM'}
              </button>
            ) : (
              <>
                <button
                  onClick={() => navigate('/immersive')}
                  className="flex-1 py-2 bg-lagado-purple/20 border border-lagado-purple/40 text-lagado-purple rounded-lg text-sm font-semibold hover:bg-lagado-purple/30 transition-all"
                >
                  Open Immersive
                </button>
                <button
                  onClick={stop}
                  disabled={loading}
                  className="py-2 px-4 bg-red-500/10 border border-red-500/30 text-red-400 rounded-lg text-sm font-semibold hover:bg-red-500/20 disabled:opacity-50 transition-all"
                >
                  {loading ? 'Stopping…' : 'Stop'}
                </button>
              </>
            )}
          </div>
        </div>

        {/* Config card */}
        <div className="bg-lagado-surface border border-lagado-border rounded-xl p-5">
          <h3 className="text-lagado-text-bright font-semibold mb-3">Configuration</h3>
          <div className="space-y-2 text-xs">
            {[
              ['Image path', '~/.laputa-secure/vm-images/Arch-Linux-x86_64-cloudimg.qcow2'],
              ['Cloud-init', '~/.laputa-secure/vm-images/seed.iso (first-boot)'],
              ['QMP socket', '/tmp/lagado-qmp.sock'],
              ['Display', 'QMP screendump → Immersive feed'],
              ['Input', 'SSH → xdotool (DISPLAY=:0)'],
              ['Perception', 'SSH → perceive.py (AT-SPI2)'],
            ].map(([k, v]) => (
              <div key={k} className="flex gap-3">
                <span className="text-lagado-text-dim w-28 flex-shrink-0">{k}</span>
                <span className="text-lagado-text font-mono text-[11px] break-all">{v}</span>
              </div>
            ))}
          </div>
        </div>

      </div>
    </div>
  )
}
