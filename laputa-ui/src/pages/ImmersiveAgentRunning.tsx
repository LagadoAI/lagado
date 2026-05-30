 
import React from "react";
import { Button } from "../components/Button";
 
export default function ImmersiveAgentRunning() {
  return (
    <div className="min-h-screen bg-black relative overflow-hidden">
      {/* VM Display (visible, agent executing) */}
      <div className="w-full h-screen bg-gradient-to-br from-gray-900 to-black flex items-center justify-center">
        <div className="text-center">
          <div className="text-laputa-green text-3xl mb-4">⚡</div>
          <p className="text-body text-laputa-text">Agent executing...</p>
          <p className="text-caption text-laputa-text-dim mt-2 font-mono">
            Action: click on element [Open Firefox]
          </p>
        </div>
      </div>
 
      {/* Glass Prompt Box (back to transparent) */}
      <div className="absolute bottom-6 left-1/2 transform -translate-x-1/2 w-full max-w-xl px-4">
        <div
          className="bg-laputa-glass-trans backdrop-blur-xl border border-laputa-border-light rounded-sm p-4 shadow-2xl"
          style={{ backdropFilter: "blur(24px) saturate(120%)" }}
        >
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-3 flex-1">
              <span className="text-xl animate-pulse">⏳</span>
              <span className="text-body text-laputa-text">Executing: click...</span>
            </div>
            <div className="flex gap-2">
              <Button variant="secondary" size="sm">
                Stop
              </Button>
              <Button variant="secondary" size="sm">
                Hide
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
