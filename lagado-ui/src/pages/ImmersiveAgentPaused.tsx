 
import React from "react";
import { Button } from "../components/Button";
 
export default function ImmersiveAgentPaused() {
  return (
    <div className="min-h-screen bg-black relative overflow-hidden">
      {/* VM Display (frozen with darker overlay) */}
      <div className="w-full h-screen bg-gradient-to-br from-gray-900 to-black opacity-60 flex items-center justify-center relative">
        <div className="text-center">
          <p className="text-body text-lagado-text-dim">[VM Display - Frozen]</p>
        </div>
        {/* Pause overlay */}
        <div className="absolute top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2">
          <div className="bg-lagado-bg bg-opacity-80 backdrop-blur-md rounded-md px-6 py-4 border border-lagado-red">
            <div className="text-lagado-red text-4xl text-center mb-2">⏸</div>
            <p className="text-h3 text-lagado-text-bright font-bold">PAUSED</p>
          </div>
        </div>
      </div>
 
      {/* Glass Prompt Box (paused state) */}
      <div className="absolute bottom-6 left-1/2 transform -translate-x-1/2 w-full max-w-xl px-4">
        <div className="bg-lagado-glass-opaque border border-lagado-red rounded-sm p-4 shadow-2xl">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-3">
              <span className="text-lagado-red text-xl">🛑</span>
              <span className="text-body text-lagado-text-bright font-semibold">
                Agent Paused
              </span>
            </div>
            <div className="flex gap-2">
              <Button variant="primary" size="sm">
                Resume
              </Button>
              <Button variant="secondary" size="sm">
                Open Side Pane
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
