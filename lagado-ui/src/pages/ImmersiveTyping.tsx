 
import React, { useState } from "react";
import { Button } from "../components/Button";
 
export default function ImmersiveTyping() {
  const [goal, setGoal] = useState("Open Firefox and search for hello world");
 
  return (
    <div className="min-h-screen bg-black relative overflow-hidden">
      {/* VM Display Area (dimmed) */}
      <div className="w-full h-screen bg-gradient-to-br from-gray-900 to-black opacity-50 flex items-center justify-center">
        <p className="text-body text-lagado-text-dim">[VM Display - Dimmed]</p>
      </div>
 
      {/* Glass Prompt Box (opaque) */}
      <div className="absolute bottom-6 left-1/2 transform -translate-x-1/2 w-full max-w-xl px-4">
        <div
          className="bg-lagado-glass-opaque border border-lagado-red rounded-sm p-4 shadow-2xl"
          style={{ transform: "scale(1.05)" }}
        >
          <div className="flex items-start gap-3 mb-3">
            <span className="text-lagado-red text-xl font-bold mt-1">/</span>
            <textarea
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              placeholder="Type your goal..."
              autoFocus
              rows={2}
              className="flex-1 bg-transparent text-lagado-text-bright outline-none resize-none placeholder-lagado-text-dim text-body"
            />
          </div>
          <div className="flex justify-end gap-2 pt-2 border-t border-lagado-border">
            <Button variant="secondary" size="sm">
              Cancel
            </Button>
            <Button variant="primary" size="sm">
              Send →
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
