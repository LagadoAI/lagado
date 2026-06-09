import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'

interface SignupPageProps {
  onSignup: () => void
}

export default function SignupPage({ onSignup }: SignupPageProps) {
  const navigate = useNavigate()
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [recovery, setRecovery] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [step, setStep] = useState<'password' | 'recovery'>('password')

  const nextStep = () => {
    if (password.length < 8) { setError('Password must be at least 8 characters'); return }
    if (password !== confirm) { setError('Passwords do not match'); return }
    setError(null)
    setStep('recovery')
  }

  const handleSignup = async () => {
    if (recovery.length < 12) { setError('Recovery phrase must be at least 12 characters'); return }
    setLoading(true)
    setError(null)
    try {
      await invoke('auth_signup', { password, recoveryPhrase: recovery })
      onSignup()
      navigate('/setup/welcome')
    } catch (e: any) {
      setError(e?.toString() ?? 'Signup failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
      <div className="w-full max-w-md">
        <div className="text-center mb-8">
          <h1 style={{ fontSize: '48px' }} className="text-lagado-text-bright font-bold tracking-wider mb-2">
            LAGADO
          </h1>
          <p className="text-sm text-lagado-text-dim">Create your encrypted vault</p>
        </div>

        <div className="bg-lagado-surface border border-lagado-border rounded-xl p-6">
          {/* Step indicator */}
          <div className="flex items-center gap-2 mb-6">
            <div className={`flex-1 h-0.5 rounded-full ${step === 'password' ? 'bg-lagado-blue' : 'bg-lagado-blue'}`} />
            <div className={`flex-1 h-0.5 rounded-full ${step === 'recovery' ? 'bg-lagado-blue' : 'bg-lagado-border'}`} />
          </div>

          {error && (
            <div className="mb-4 px-3 py-2 bg-red-500/10 border border-red-500/30 rounded-lg">
              <p className="text-xs text-red-400">{error}</p>
            </div>
          )}

          {step === 'password' ? (
            <div className="space-y-4">
              <div>
                <label className="block text-xs text-lagado-text-dim mb-1.5">Password</label>
                <input
                  type="password"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  placeholder="At least 8 characters"
                  className="w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-lg text-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-blue focus:outline-none transition-colors"
                />
              </div>
              <div>
                <label className="block text-xs text-lagado-text-dim mb-1.5">Confirm password</label>
                <input
                  type="password"
                  value={confirm}
                  onChange={e => setConfirm(e.target.value)}
                  placeholder="Repeat password"
                  onKeyDown={e => e.key === 'Enter' && nextStep()}
                  className="w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-lg text-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-blue focus:outline-none transition-colors"
                />
              </div>
              <button
                onClick={nextStep}
                className="w-full py-2.5 bg-lagado-blue/20 border border-lagado-blue/40 text-lagado-blue rounded-lg text-sm font-semibold hover:bg-lagado-blue/30 transition-all"
              >
                Continue
              </button>
            </div>
          ) : (
            <div className="space-y-4">
              <div className="px-3 py-2.5 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
                <p className="text-xs text-yellow-400 font-medium mb-1">Write this down and store it safely</p>
                <p className="text-xs text-yellow-300/70">Your recovery phrase is the only way to regain access if you forget your password. It is never stored in the cloud.</p>
              </div>
              <div>
                <label className="block text-xs text-lagado-text-dim mb-1.5">Recovery phrase</label>
                <textarea
                  value={recovery}
                  onChange={e => setRecovery(e.target.value)}
                  placeholder="Enter a memorable passphrase (min 12 characters)"
                  rows={3}
                  className="w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-lg text-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-purple focus:outline-none transition-colors resize-none"
                />
              </div>
              <div className="flex gap-2">
                <button
                  onClick={() => { setStep('password'); setError(null) }}
                  className="px-4 py-2.5 bg-lagado-surface-2 border border-lagado-border text-lagado-text-dim rounded-lg text-sm hover:text-lagado-text transition-colors"
                >
                  Back
                </button>
                <button
                  onClick={handleSignup}
                  disabled={loading}
                  className="flex-1 py-2.5 bg-lagado-purple/20 border border-lagado-purple/40 text-lagado-purple rounded-lg text-sm font-semibold hover:bg-lagado-purple/30 disabled:opacity-50 transition-all"
                >
                  {loading ? 'Creating vault…' : 'Create vault'}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
