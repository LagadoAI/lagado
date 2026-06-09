import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'

interface ServerStatus {
  running: boolean
  model: string
  host: string
  port: number
  endpoint: string
}

export default function ServerManagement() {
  const navigate = useNavigate()
  const [status, setStatus] = useState<ServerStatus | null>(null)
  const [checking, setChecking] = useState(true)

  const check = () => {
    setChecking(true)
    invoke<ServerStatus>('get_server_status')
      .then(s => { setStatus(s); setChecking(false) })
      .catch(() => setChecking(false))
  }

  useEffect(() => { check() }, [])

  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <div className="border-b border-lagado-border bg-lagado-surface px-4 py-3 flex items-center gap-3">
        <button onClick={() => navigate('/chat')} className="px-3 py-1.5 text-body-sm text-lagado-text-dim hover:text-lagado-text border border-lagado-border rounded-md hover:border-lagado-blue transition-colors">
          ← Chat
        </button>
        <h1 className="text-h3 text-lagado-text-bright font-semibold">Server</h1>
      </div>

      <div className="flex-1 p-6 max-w-2xl">
        <div className="bg-lagado-surface border border-lagado-border rounded-md p-6 mb-4">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-h3 text-lagado-text-bright font-semibold">llama-server</h2>
            <div className="flex items-center gap-2">
              <span className={`w-2 h-2 rounded-full ${checking ? 'bg-lagado-yellow' : status?.running ? 'bg-lagado-green' : 'bg-lagado-red'}`} />
              <span className="text-body-sm text-lagado-text-dim">
                {checking ? 'Checking...' : status?.running ? 'Running' : 'Stopped'}
              </span>
            </div>
          </div>

          {status && (
            <div className="space-y-3 text-body-sm">
              <div className="flex justify-between">
                <span className="text-lagado-text-dim">Model</span>
                <span className="font-mono text-lagado-text">{status.model}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-lagado-text-dim">Endpoint</span>
                <span className="font-mono text-lagado-blue">{status.endpoint}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-lagado-text-dim">Port</span>
                <span className="font-mono text-lagado-text">{status.port}</span>
              </div>
            </div>
          )}

          <div className="flex gap-3 mt-6">
            <button onClick={check} className="px-4 py-2 border border-lagado-border text-lagado-text-dim text-body-sm rounded-md hover:border-lagado-blue hover:text-lagado-text transition-colors">
              Refresh
            </button>
            <button
              onClick={() => navigate('/settings')}
              className="px-4 py-2 border border-lagado-border text-lagado-text-dim text-body-sm rounded-md hover:border-lagado-blue hover:text-lagado-text transition-colors"
            >
              Change Model
            </button>
          </div>
        </div>

        <p className="text-caption text-lagado-text-dim">
          llama-server is managed automatically by Lagado. It starts when you launch the app and stops when you close it.
        </p>
      </div>
    </div>
  )
}
