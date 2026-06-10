import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'

interface SignupPageProps {
  onSignup: () => void
}

export default function SignupPage({ onSignup }: SignupPageProps) {
  const navigate = useNavigate()
  const [step, setStep] = useState<'identity' | 'password' | 'recovery'>('identity')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [recovery, setRecovery] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const goToPassword = () => {
    if (!username.trim()) { setError('Enter a username'); return }
    setError(null); setStep('password')
  }

  const goToRecovery = () => {
    if (password.length < 8) { setError('Password must be at least 8 characters'); return }
    if (password !== confirm) { setError('Passwords do not match'); return }
    setError(null); setStep('recovery')
  }

  const handleSignup = async () => {
    if (recovery.length < 12) { setError('Recovery phrase must be at least 12 characters'); return }
    setLoading(true); setError(null)
    try {
      await invoke('auth_signup', { password, recoveryPhrase: recovery })
      localStorage.setItem('lagado_username', username.trim())
      onSignup()
      navigate('/setup/welcome')
    } catch (e: any) {
      setError(e?.toString() ?? 'Signup failed')
    } finally { setLoading(false) }
  }

  const steps = ['identity', 'password', 'recovery']
  const stepIdx = steps.indexOf(step)

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
          <h1 style={{ fontSize: 44, fontWeight: 700, letterSpacing: '.18em', textTransform: 'uppercase', color: 'var(--text-strong)', margin: '12px 0 6px', fontFamily: 'var(--font-display)' }}>
            Lagado
          </h1>
          <p style={{ fontSize: 13, color: 'var(--text-dim)', margin: 0 }}>Create your encrypted vault</p>
        </div>

        <div className="lg-card" style={{ padding: 22 }}>
          {/* Step bar */}
          <div style={{ display: 'flex', gap: 4, marginBottom: 20 }}>
            {steps.map((s, i) => (
              <div key={s} style={{ flex: 1, height: 3, borderRadius: 99, background: i <= stepIdx ? 'var(--blue-500)' : 'var(--line-700)', transition: 'background .3s' }} />
            ))}
          </div>

          {error && (
            <div style={{ marginBottom: 14, padding: '8px 12px', background: 'var(--red-dim)', border: '1px solid rgba(239,68,68,.3)', borderRadius: 8 }}>
              <p style={{ fontSize: 12, color: 'var(--red-500)' }}>{error}</p>
            </div>
          )}

          {step === 'identity' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <label className="lg-label">Choose a username</label>
                <input
                  className="lg-field"
                  value={username}
                  onChange={e => setUsername(e.target.value)}
                  placeholder="local_user"
                  onKeyDown={e => e.key === 'Enter' && goToPassword()}
                  autoFocus
                />
                <p style={{ fontSize: 11, color: 'var(--text-dim)', marginTop: 6 }}>
                  Stored locally only. Used for display.
                </p>
              </div>
              <button className="lg-btn lg-btn--primary lg-btn--lg" style={{ width: '100%', marginTop: 6 }} onClick={goToPassword}>
                Continue
              </button>
            </div>
          )}

          {step === 'password' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <label className="lg-label">Passphrase</label>
                <input className="lg-field" type="password" value={password} onChange={e => setPassword(e.target.value)} placeholder="At least 8 characters" />
              </div>
              <div>
                <label className="lg-label">Confirm passphrase</label>
                <input className="lg-field" type="password" value={confirm} onChange={e => setConfirm(e.target.value)} placeholder="Repeat passphrase" onKeyDown={e => e.key === 'Enter' && goToRecovery()} />
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button className="lg-btn lg-btn--ghost lg-btn--md" onClick={() => { setStep('identity'); setError(null) }}>Back</button>
                <button className="lg-btn lg-btn--primary lg-btn--md" style={{ flex: 1 }} onClick={goToRecovery}>Continue</button>
              </div>
            </div>
          )}

          {step === 'recovery' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div style={{ padding: '10px 14px', background: 'var(--yellow-dim)', border: '1px solid rgba(245,158,11,.3)', borderRadius: 8 }}>
                <p style={{ fontSize: 12, color: 'var(--yellow-500)', fontWeight: 600, marginBottom: 4 }}>Write this down and store it safely</p>
                <p style={{ fontSize: 12, color: 'rgba(245,158,11,.7)', lineHeight: 1.5 }}>
                  Your recovery phrase is the only way to regain access if you forget your passphrase. Never stored in the cloud.
                </p>
              </div>
              <div>
                <label className="lg-label">Recovery phrase</label>
                <textarea
                  className="lg-field"
                  value={recovery}
                  onChange={e => setRecovery(e.target.value)}
                  placeholder="Enter a memorable passphrase (min 12 characters)"
                  rows={3}
                  style={{ height: 'auto', padding: '10px 12px' }}
                />
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button className="lg-btn lg-btn--ghost lg-btn--md" onClick={() => { setStep('password'); setError(null) }}>Back</button>
                <button className="lg-btn lg-btn--primary lg-btn--md" style={{ flex: 1 }} onClick={handleSignup} disabled={loading}>
                  {loading ? 'Creating vault…' : 'Create vault'}
                </button>
              </div>
            </div>
          )}
        </div>

        <p style={{ textAlign: 'center', marginTop: 18, fontSize: 11, color: 'var(--text-dim)' }}>
          AES-256-GCM · Argon2id · Local authentication only
        </p>
      </div>
    </div>
  )
}
