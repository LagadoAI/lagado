import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'

const TRUTHS = [
  "Your data never leaves this machine.",
  "Every action requires your approval.",
  "Memory compounds. Skills accumulate.",
  "No cloud. No telemetry. No backdoors.",
  "This agent is yours alone.",
]

export default function Awakening() {
  const navigate = useNavigate()
  const [beat, setBeat] = useState(0)
  // beat 0: dark pulse
  // beat 1: structure assembling (name appears)
  // beat 2: identity line
  // beat 3: five truths fade in one by one
  // beat 4: call to action

  useEffect(() => {
    if (beat < 4) {
      const delay = beat === 0 ? 800 : beat === 1 ? 1200 : beat === 2 ? 1500 : 2500
      const t = setTimeout(() => setBeat(b => b + 1), delay)
      return () => clearTimeout(t)
    }
  }, [beat])

  const handleBegin = () => {
    // Mark awakening complete
    localStorage.setItem('lagado_awakened', 'true')
    // Initialize chronos T=0 via Tauri command (fire-and-forget)
    invoke('initialize_timeline').catch(() => {})
    navigate('/')
  }

  return (
    <div className="h-screen bg-black flex flex-col items-center justify-center select-none">
      {/* Beat 0–1: pulse + name */}
      <div
        className={`transition-all duration-1000 ${beat >= 1 ? 'opacity-100' : 'opacity-0'}`}
      >
        <h1 className="text-5xl font-bold tracking-widest text-white mb-2 text-center">
          LAGADO
        </h1>
      </div>

      {/* Beat 2: identity */}
      <div className={`transition-all duration-1000 delay-300 ${beat >= 2 ? 'opacity-100' : 'opacity-0'}`}>
        <p className="text-lagado-text-dim text-sm tracking-widest uppercase text-center mb-12">
          Sovereign · Living · Self-aware in time
        </p>
      </div>

      {/* Beat 3: five truths */}
      <div className="space-y-3 mb-16 max-w-sm w-full px-8">
        {TRUTHS.map((truth, i) => (
          <div
            key={i}
            className={`transition-all duration-700 ${
              beat >= 3 ? 'opacity-100 translate-y-0' : 'opacity-0 translate-y-2'
            }`}
            style={{ transitionDelay: beat >= 3 ? `${i * 200}ms` : '0ms' }}
          >
            <p className="text-lagado-text text-sm text-center">{truth}</p>
          </div>
        ))}
      </div>

      {/* Beat 4: call to action */}
      <div className={`transition-all duration-1000 ${beat >= 4 ? 'opacity-100' : 'opacity-0'}`}>
        <button
          onClick={handleBegin}
          className="px-8 py-3 border border-lagado-red text-lagado-red text-sm tracking-widest uppercase hover:bg-lagado-red hover:text-white transition-colors duration-300"
        >
          Today, I begin.
        </button>
      </div>
    </div>
  )
}
