import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface LoginPageProps {
  onLogin: () => void
  onSignup?: () => void
}

export default function LoginPage({ onLogin, onSignup }: LoginPageProps) {
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [lockedSecs, setLockedSecs] = useState(0)
  const [showRecovery, setShowRecovery] = useState(false)
  const [recoveryPhrase, setRecoveryPhrase] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [newConfirm, setNewConfirm] = useState('')

  const username = localStorage.getItem('lagado_username') || 'local_user'

  useEffect(() => {
    if (lockedSecs <= 0) return
    const id = setInterval(() => setLockedSecs(s => Math.max(0, s - 1)), 1000)
    return () => clearInterval(id)
  }, [lockedSecs > 0])

  const handleLogin = async () => {
    if (!password) { setError('Enter your passphrase'); return }
    setLoading(true); setError(null)
    try {
      await invoke('auth_login', { password })
      onLogin()
    } catch (e: any) {
      const msg = e?.toString() ?? ''
      if (msg.startsWith('locked:')) {
        setLockedSecs(parseInt(msg.split(':')[1]) || 600); setError(null)
      } else if (msg.startsWith('wrong_password:')) {
        const left = msg.split(':')[1]
        setError(`Wrong passphrase — ${left} attempt${left === '1' ? '' : 's'} remaining`)
      } else {
        setError(msg || 'Login failed')
      }
    } finally { setLoading(false) }
  }

  const handleRecover = async () => {
    if (!recoveryPhrase) { setError('Enter your recovery phrase'); return }
    if (newPassword.length < 8) { setError('Password must be at least 8 characters'); return }
    if (newPassword !== newConfirm) { setError('Passwords do not match'); return }
    setLoading(true); setError(null)
    try {
      await invoke('auth_recover', { recoveryPhrase, newPassword })
      onLogin()
    } catch (e: any) {
      setError(e?.toString() ?? 'Recovery failed')
    } finally { setLoading(false) }
  }

  const formatTime = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`

  return (
    <div style={{
      height: '100vh', display: 'flex', alignItems: 'center', justifyContent: 'center',
      padding: 16,
      background: 'radial-gradient(700px 380px at 50% 8%, rgba(139,92,246,.16), transparent 60%), var(--bg)',
    }}>
      <div style={{ width: 380 }}>
        {/* Logo lockup */}
        <div style={{ textAlign: 'center', marginBottom: 26 }}>
          <img src="/lagado-mark.png" width={56} height={56} alt="Lagado"
            style={{ filter: 'drop-shadow(0 0 18px rgba(139,92,246,.45))', display: 'inline-block' }} />
          <h1 style={{
            fontSize: 44, fontWeight: 700, letterSpacing: '.18em', textTransform: 'uppercase',
            color: 'var(--text-strong)', margin: '12px 0 6px', fontFamily: 'var(--font-display)',
          }}>Lagado</h1>
          <p style={{ fontSize: 13, color: 'var(--text-dim)', margin: 0 }}>Local • Private • Yours</p>
        </div>

        {/* Auth card */}
        <div className="lg-card" style={{ padding: 22 }}>
          {lockedSecs > 0 && (
            <div style={{ marginBottom: 16, padding: '10px 14px', background: 'var(--red-dim)', border: '1px solid rgba(239,68,68,.3)', borderRadius: 8, textAlign: 'center' }}>
              <p style={{ fontSize: 12, color: 'var(--red-500)', fontWeight: 600 }}>Too many failed attempts</p>
              <p style={{ fontSize: 20, fontFamily: 'var(--font-mono)', color: '#ef4444', margin: '4px 0 2px' }}>{formatTime(lockedSecs)}</p>
              <p style={{ fontSize: 11, color: 'rgba(239,68,68,.6)' }}>Try again after timer expires</p>
            </div>
          )}

          {error && (
            <div style={{ marginBottom: 14, padding: '8px 12px', background: 'var(--red-dim)', border: '1px solid rgba(239,68,68,.3)', borderRadius: 8 }}>
              <p style={{ fontSize: 12, color: 'var(--red-500)' }}>{error}</p>
            </div>
          )}

          {!showRecovery ? (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <label className="lg-label">Username</label>
                <input className="lg-field" value={username} readOnly style={{ cursor: 'default', opacity: 0.7 }} />
              </div>
              <div>
                <label className="lg-label">Passphrase</label>
                <input
                  className="lg-field"
                  type="password"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  onKeyDown={e => e.key === 'Enter' && handleLogin()}
                  placeholder="Enter passphrase"
                  disabled={lockedSecs > 0}
                />
              </div>
              <button
                className="lg-btn lg-btn--primary lg-btn--lg"
                style={{ width: '100%', marginTop: 6 }}
                onClick={handleLogin}
                disabled={loading || lockedSecs > 0}
              >
                {loading ? 'Unlocking…' : 'Unlock'}
              </button>
              <div style={{ marginTop: 8, paddingTop: 14, borderTop: '1px solid var(--line-700)', textAlign: 'center', display: 'flex', flexDirection: 'column', gap: 8 }}>
                <button
                  onClick={() => { setShowRecovery(true); setError(null) }}
                  style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-dim)', fontSize: 12, fontFamily: 'var(--font-display)' }}
                >
                  Forgot passphrase? Use recovery phrase
                </button>
                {onSignup && (
                  <button
                    onClick={onSignup}
                    style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--purple-500)', fontSize: 12, fontFamily: 'var(--font-display)' }}
                  >
                    First time? Create account
                  </button>
                )}
              </div>
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <p style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 4 }}>
                Enter your recovery phrase to set a new passphrase.
              </p>
              <div>
                <label className="lg-label">Recovery phrase</label>
                <textarea
                  className="lg-field"
                  value={recoveryPhrase}
                  onChange={e => setRecoveryPhrase(e.target.value)}
                  placeholder="Your recovery phrase"
                  rows={2}
                  style={{ height: 'auto', padding: '10px 12px' }}
                />
              </div>
              <div>
                <label className="lg-label">New passphrase</label>
                <input className="lg-field" type="password" value={newPassword} onChange={e => setNewPassword(e.target.value)} placeholder="At least 8 characters" />
              </div>
              <div>
                <label className="lg-label">Confirm new passphrase</label>
                <input className="lg-field" type="password" value={newConfirm} onChange={e => setNewConfirm(e.target.value)} placeholder="Repeat passphrase" onKeyDown={e => e.key === 'Enter' && handleRecover()} />
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button className="lg-btn lg-btn--ghost lg-btn--md" onClick={() => { setShowRecovery(false); setError(null) }}>
                  Back
                </button>
                <button className="lg-btn lg-btn--primary lg-btn--md" style={{ flex: 1 }} onClick={handleRecover} disabled={loading}>
                  {loading ? 'Recovering…' : 'Recover vault'}
                </button>
              </div>
            </div>
          )}
        </div>

        <p style={{ textAlign: 'center', marginTop: 18, fontSize: 11, color: 'var(--text-dim)' }}>
          Encrypted with AES-256 · Local authentication only
        </p>
      </div>
    </div>
  )
}
