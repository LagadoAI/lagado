import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'

interface LoginPageProps {
  onLogin: () => void
}

export default function LoginPage({ onLogin }: LoginPageProps) {
  const navigate = useNavigate()
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [lockedSecs, setLockedSecs] = useState(0)
  const [showRecovery, setShowRecovery] = useState(false)
  const [recoveryPhrase, setRecoveryPhrase] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [newConfirm, setNewConfirm] = useState('')

  // Countdown timer when locked
  useEffect(() => {
    if (lockedSecs <= 0) return
    const id = setInterval(() => setLockedSecs(s => Math.max(0, s - 1)), 1000)
    return () => clearInterval(id)
  }, [lockedSecs > 0])

  const handleLogin = async () => {
    if (!password) { setError('Enter your password'); return }
    setLoading(true)
    setError(null)
    try {
      await invoke('auth_login', { password })
      onLogin()
      navigate('/chat')
    } catch (e: any) {
      const msg = e?.toString() ?? ''
      if (msg.startsWith('locked:')) {
        setLockedSecs(parseInt(msg.split(':')[1]) || 600)
        setError(null)
      } else if (msg.startsWith('wrong_password:')) {
        const left = msg.split(':')[1]
        setError(`Wrong password — ${left} attempt${left === '1' ? '' : 's'} remaining`)
      } else {
        setError(msg || 'Login failed')
      }
    } finally {
      setLoading(false)
    }
  }

  const handleRecover = async () => {
    if (!recoveryPhrase) { setError('Enter your recovery phrase'); return }
    if (newPassword.length < 8) { setError('Password must be at least 8 characters'); return }
    if (newPassword !== newConfirm) { setError('Passwords do not match'); return }
    setLoading(true)
    setError(null)
    try {
      await invoke('auth_recover', { recoveryPhrase, newPassword })
      onLogin()
      navigate('/chat')
    } catch (e: any) {
      setError(e?.toString() ?? 'Recovery failed')
    } finally {
      setLoading(false)
    }
  }

  const formatTime = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`

  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
      <div className="w-full max-w-md">
        <div className="text-center mb-8">
          <h1 style={{ fontSize: '72px' }} className="text-lagado-text-bright font-bold tracking-wider mb-2">
            LAGADO
          </h1>
          <p className="text-sm text-lagado-text-dim">Local · Private · Yours</p>
        </div>

        <div className="bg-lagado-surface border border-lagado-border rounded-xl p-6">
          {lockedSecs > 0 && (
            <div className="mb-4 px-3 py-3 bg-red-500/10 border border-red-500/30 rounded-lg text-center">
              <p className="text-xs text-red-400 font-medium">Too many failed attempts</p>
              <p className="text-xl font-mono text-red-300 mt-1">{formatTime(lockedSecs)}</p>
              <p className="text-xs text-red-400/60 mt-1">Try again after the timer expires</p>
            </div>
          )}

          {error && (
            <div className="mb-4 px-3 py-2 bg-red-500/10 border border-red-500/30 rounded-lg">
              <p className="text-xs text-red-400">{error}</p>
            </div>
          )}

          {!showRecovery ? (
            <div className="space-y-4">
              <div>
                <label className="block text-xs text-lagado-text-dim mb-1.5">Password</label>
                <input
                  type="password"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  onKeyDown={e => e.key === 'Enter' && handleLogin()}
                  placeholder="Enter your password"
                  disabled={lockedSecs > 0}
                  className="w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-lg text-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-blue focus:outline-none disabled:opacity-40 transition-colors"
                />
              </div>
              <button
                onClick={handleLogin}
                disabled={loading || lockedSecs > 0}
                className="w-full py-2.5 bg-lagado-blue/20 border border-lagado-blue/40 text-lagado-blue rounded-lg text-sm font-semibold hover:bg-lagado-blue/30 disabled:opacity-50 transition-all"
              >
                {loading ? 'Unlocking…' : 'Unlock'}
              </button>
              <button
                onClick={() => { setShowRecovery(true); setError(null) }}
                className="w-full text-xs text-lagado-text-dim hover:text-lagado-text transition-colors py-1"
              >
                Forgot password? Use recovery phrase
              </button>
            </div>
          ) : (
            <div className="space-y-4">
              <p className="text-xs text-lagado-text-dim mb-2">Enter your recovery phrase to set a new password.</p>
              <div>
                <label className="block text-xs text-lagado-text-dim mb-1.5">Recovery phrase</label>
                <textarea
                  value={recoveryPhrase}
                  onChange={e => setRecoveryPhrase(e.target.value)}
                  placeholder="Your recovery phrase"
                  rows={2}
                  className="w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-lg text-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-purple focus:outline-none resize-none transition-colors"
                />
              </div>
              <div>
                <label className="block text-xs text-lagado-text-dim mb-1.5">New password</label>
                <input
                  type="password"
                  value={newPassword}
                  onChange={e => setNewPassword(e.target.value)}
                  placeholder="At least 8 characters"
                  className="w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-lg text-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-blue focus:outline-none transition-colors"
                />
              </div>
              <div>
                <label className="block text-xs text-lagado-text-dim mb-1.5">Confirm new password</label>
                <input
                  type="password"
                  value={newConfirm}
                  onChange={e => setNewConfirm(e.target.value)}
                  placeholder="Repeat password"
                  onKeyDown={e => e.key === 'Enter' && handleRecover()}
                  className="w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-lg text-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-blue focus:outline-none transition-colors"
                />
              </div>
              <div className="flex gap-2">
                <button
                  onClick={() => { setShowRecovery(false); setError(null) }}
                  className="px-4 py-2.5 bg-lagado-surface-2 border border-lagado-border text-lagado-text-dim rounded-lg text-sm hover:text-lagado-text transition-colors"
                >
                  Back
                </button>
                <button
                  onClick={handleRecover}
                  disabled={loading}
                  className="flex-1 py-2.5 bg-lagado-purple/20 border border-lagado-purple/40 text-lagado-purple rounded-lg text-sm font-semibold hover:bg-lagado-purple/30 disabled:opacity-50 transition-all"
                >
                  {loading ? 'Recovering…' : 'Recover vault'}
                </button>
              </div>
            </div>
          )}
        </div>

        <p className="text-center mt-6 text-xs text-lagado-text-dim">
          AES-256-GCM · Argon2id · Local only
        </p>
      </div>
    </div>
  )
}
