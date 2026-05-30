 
import React, { useState } from "react";
import { useNavigate } from "react-router-dom";
 
export default function ImmersiveDefault() {
  const navigate = useNavigate();
  const [showMenu, setShowMenu] = useState(false);
 
  return (
    <div className="min-h-screen bg-black relative overflow-hidden">
      {/* Hamburger menu (top-left) */}
      <button
        onClick={() => setShowMenu(!showMenu)}
        className="absolute top-4 left-4 z-50 text-laputa-text-bright text-2xl hover:text-laputa-red transition-colors p-2 bg-laputa-surface bg-opacity-30 rounded backdrop-blur-md"
      >
        ≡
      </button>
 
      {/* VM Display Area */}
      <div className="w-full h-screen flex items-center justify-center bg-gradient-to-br from-gray-900 to-black">
        <div className="text-center">
          <div className="w-32 h-32 mx-auto mb-6 bg-laputa-purple-mid rounded-lg flex items-center justify-center">
            <span className="text-6xl">⚔</span>
          </div>
          <p className="text-body text-laputa-text-dim">
            [VM Display - Awaiting input]
          </p>
        </div>
      </div>
 
      {/* Glass Prompt Box (transparent) */}
      <div
        className="absolute bottom-6 left-1/2 transform -translate-x-1/2 w-full max-w-xl px-4"
      >
        <div
          className="bg-laputa-glass-trans backdrop-blur-xl border border-laputa-border-light rounded-sm p-4 shadow-2xl"
          style={{ backdropFilter: "blur(24px) saturate(120%)" }}
        >
          <div className="flex items-center gap-3">
            <span className="text-laputa-red text-xl font-bold">/</span>
            <span className="text-body text-laputa-text-dim flex-1">
              Type your goal...
            </span>
            <span className="text-caption text-laputa-text-dim font-mono">⏎ to send</span>
          </div>
        </div>
      </div>
 
      {/* Menu overlay (when opened) */}
      {showMenu && (
        <div className="absolute top-16 left-4 z-50 bg-laputa-surface border border-laputa-border rounded-sm py-2 min-w-[200px] shadow-xl">
          <button
            onClick={() => navigate("/chat")}
            className="w-full text-left px-4 py-2 text-body text-laputa-text hover:bg-laputa-surface-2 transition-colors"
          >
            ← Return to Chat
          </button>
          <hr className="border-laputa-border my-2" />
          <button className="w-full text-left px-4 py-2 text-body text-laputa-text hover:bg-laputa-surface-2 transition-colors">
            ⏸ Pause Agent
          </button>
          <button className="w-full text-left px-4 py-2 text-body text-laputa-text hover:bg-laputa-surface-2 transition-colors">
            ⚙ Settings
          </button>
        </div>
      )}
    </div>
  );
}
 
