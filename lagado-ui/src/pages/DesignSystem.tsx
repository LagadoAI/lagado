// Design system showcase — component canvas for live visual iteration
// Run the app and navigate to /design to preview all components

import { useState } from 'react'

export default function DesignSystem() {
  const [input, setInput] = useState('')

  return (
    <div className="min-h-screen bg-lagado-bg p-8 space-y-12">
      <div>
        <h1 className="text-4xl font-bold bg-gradient-to-r from-lagado-blue to-lagado-purple bg-clip-text text-transparent mb-2">
          Design System
        </h1>
        <p className="text-lagado-text-dim">Live component canvas — edit components and see them here</p>
      </div>

      {/* Colors */}
      <section>
        <h2 className="text-h2 text-lagado-text-bright font-semibold mb-4">Colors</h2>
        <div className="flex gap-3 flex-wrap">
          {[
            { name: 'bg', color: 'bg-lagado-bg border border-lagado-border' },
            { name: 'surface', color: 'bg-lagado-surface' },
            { name: 'surface-2', color: 'bg-lagado-surface-2' },
            { name: 'blue', color: 'bg-lagado-blue' },
            { name: 'purple', color: 'bg-lagado-purple' },
            { name: 'green', color: 'bg-lagado-green' },
            { name: 'red', color: 'bg-lagado-red' },
            { name: 'yellow', color: 'bg-lagado-yellow' },
          ].map(({ name, color }) => (
            <div key={name} className="flex flex-col items-center gap-1">
              <div className={`w-12 h-12 rounded-lg ${color}`} />
              <span className="text-caption text-lagado-text-dim">{name}</span>
            </div>
          ))}
        </div>
      </section>

      {/* Typography */}
      <section>
        <h2 className="text-h2 text-lagado-text-bright font-semibold mb-4">Typography</h2>
        <div className="space-y-3">
          <p className="text-4xl font-bold bg-gradient-to-r from-lagado-blue to-lagado-purple bg-clip-text text-transparent">Gradient headline</p>
          <p className="text-h1 text-lagado-text-bright font-bold">H1 — Bright text</p>
          <p className="text-h2 text-lagado-text font-semibold">H2 — Normal text</p>
          <p className="text-body text-lagado-text">Body — The quick brown fox jumps over the lazy dog</p>
          <p className="text-body-sm text-lagado-text-dim">Body SM — Dimmed secondary text</p>
          <p className="text-caption text-lagado-text-dim font-mono">Caption mono — 0x3b82f6</p>
        </div>
      </section>

      {/* Message bubbles */}
      <section>
        <h2 className="text-h2 text-lagado-text-bright font-semibold mb-4">Message Bubbles</h2>
        <div className="max-w-2xl space-y-4">
          {/* User */}
          <div className="flex justify-end">
            <div className="max-w-md bg-gradient-to-br from-lagado-blue to-lagado-purple text-white p-4 rounded-2xl rounded-tr-sm shadow-lg shadow-lagado-blue/20">
              <p className="text-body">Hey, can you open my browser?</p>
            </div>
          </div>
          {/* AI */}
          <div className="flex gap-3">
            <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-lagado-blue to-lagado-purple flex-shrink-0 flex items-center justify-center shadow-[0_0_12px_rgba(139,92,246,0.3)]">
              <span className="text-white text-caption font-bold">L</span>
            </div>
            <div className="relative bg-lagado-surface/60 backdrop-blur-md border border-lagado-border/60 rounded-2xl rounded-tl-sm p-4 max-w-md">
              <div className="absolute left-0 top-4 bottom-4 w-0.5 rounded-full bg-gradient-to-b from-lagado-blue to-lagado-purple" />
              <p className="pl-4 text-body text-lagado-text">I'll open your default browser right away. Click approve to proceed.</p>
            </div>
          </div>
          {/* Status message */}
          <div className="flex gap-3">
            <div className="w-8 h-8 rounded-lg bg-lagado-surface-2 border border-lagado-border flex-shrink-0" />
            <div className="relative bg-lagado-surface/40 border border-lagado-border/40 rounded-2xl rounded-tl-sm p-3 max-w-md">
              <div className="absolute left-0 top-3 bottom-3 w-0.5 rounded-full bg-lagado-yellow/60" />
              <p className="pl-4 text-body-sm text-lagado-text-dim italic">[goal_received] open browser</p>
            </div>
          </div>
        </div>
      </section>

      {/* Buttons */}
      <section>
        <h2 className="text-h2 text-lagado-text-bright font-semibold mb-4">Buttons</h2>
        <div className="flex gap-3 flex-wrap">
          <button className="px-4 py-2 bg-gradient-to-r from-lagado-blue to-lagado-purple text-white rounded-xl text-body-sm font-medium hover:opacity-90 hover:shadow-[0_0_12px_rgba(139,92,246,0.3)] transition-all">
            Primary
          </button>
          <button className="px-4 py-2 border border-lagado-border text-lagado-text rounded-xl text-body-sm hover:border-lagado-blue hover:shadow-[0_0_10px_rgba(59,130,246,0.15)] transition-all">
            Secondary
          </button>
          <button className="px-4 py-2 border border-lagado-red text-lagado-red rounded-xl text-body-sm hover:bg-lagado-red hover:text-white transition-all">
            Destructive
          </button>
          <button className="px-4 py-2 bg-lagado-green/20 border border-lagado-green/40 text-lagado-green rounded-xl text-body-sm hover:bg-lagado-green/30 transition-all">
            Approve
          </button>
        </div>
      </section>

      {/* Input */}
      <section>
        <h2 className="text-h2 text-lagado-text-bright font-semibold mb-4">Input</h2>
        <div className="max-w-lg">
          <div className="bg-lagado-surface/60 backdrop-blur-md border border-lagado-border/60 rounded-2xl p-3 focus-within:border-lagado-blue/60 focus-within:shadow-[0_0_20px_rgba(59,130,246,0.12)] transition-all">
            <textarea
              value={input}
              onChange={e => setInput(e.target.value)}
              placeholder="Type your message..."
              rows={2}
              className="w-full bg-transparent text-lagado-text placeholder-lagado-text-dim outline-none resize-none text-body"
            />
            <div className="flex justify-end mt-2 pt-2 border-t border-lagado-border/40">
              <button className="px-4 py-1.5 bg-gradient-to-r from-lagado-blue to-lagado-purple text-white rounded-xl text-body-sm font-medium hover:opacity-90 hover:shadow-[0_0_12px_rgba(139,92,246,0.3)] transition-all">
                Send
              </button>
            </div>
          </div>
        </div>
      </section>

      {/* Cards */}
      <section>
        <h2 className="text-h2 text-lagado-text-bright font-semibold mb-4">Cards</h2>
        <div className="grid grid-cols-3 gap-4 max-w-3xl">
          <div className="bg-lagado-surface/60 backdrop-blur-md border border-lagado-border/60 rounded-xl p-4">
            <p className="text-body-sm text-lagado-text-dim mb-1">Standard card</p>
            <p className="text-h3 text-lagado-text-bright font-semibold">Glassmorphism</p>
          </div>
          <div className="bg-lagado-surface/60 backdrop-blur-md border border-lagado-blue/40 rounded-xl p-4 shadow-[0_0_20px_rgba(59,130,246,0.1)]">
            <p className="text-body-sm text-lagado-blue mb-1">Active / focused</p>
            <p className="text-h3 text-lagado-text-bright font-semibold">Blue glow</p>
          </div>
          <div className="bg-lagado-surface/60 backdrop-blur-md border border-lagado-purple/40 rounded-xl p-4 shadow-[0_0_20px_rgba(139,92,246,0.1)]">
            <p className="text-body-sm text-lagado-purple mb-1">Secondary accent</p>
            <p className="text-h3 text-lagado-text-bright font-semibold">Purple glow</p>
          </div>
        </div>
      </section>

      {/* Permission card preview */}
      <section>
        <h2 className="text-h2 text-lagado-text-bright font-semibold mb-4">Permission Card</h2>
        <div className="max-w-md bg-lagado-surface/60 backdrop-blur-md border border-lagado-yellow/40 rounded-xl p-4 shadow-[0_0_20px_rgba(245,158,11,0.1)]">
          <div className="flex items-center gap-2 mb-3">
            <span className="w-2 h-2 rounded-full bg-lagado-yellow animate-pulse" />
            <span className="text-body-sm text-lagado-yellow font-medium">Action requires approval</span>
          </div>
          <p className="text-body text-lagado-text-bright mb-1">click(selector="ref_1")</p>
          <p className="text-body-sm text-lagado-text-dim mb-4">Write action requires confirmation</p>
          <div className="flex gap-2">
            <button className="flex-1 py-2 bg-lagado-green/20 border border-lagado-green/40 text-lagado-green rounded-lg text-body-sm hover:bg-lagado-green/30 transition-all">
              Approve
            </button>
            <button className="flex-1 py-2 bg-lagado-red/10 border border-lagado-red/40 text-lagado-red rounded-lg text-body-sm hover:bg-lagado-red/20 transition-all">
              Deny
            </button>
          </div>
        </div>
      </section>
    </div>
  )
}
