 
import React, { useState } from "react";
import { Button } from "../components/Button";
 
export default function ImmersiveWithSidebar() {
  const [activeTab, setActiveTab] = useState("reasoning");
 
  const tabs = [
    { id: "reasoning", label: "Reasoning" },
    { id: "tools", label: "Tools" },
    { id: "history", label: "History" },
    { id: "permissions", label: "Permissions" },
    { id: "settings", label: "Settings" },
  ];
 
  return (
    <div className="min-h-screen bg-black flex">
      {/* VM Display (60%) */}
      <div className="w-3/5 bg-gradient-to-br from-gray-900 to-black flex items-center justify-center">
        <div className="text-center">
          <p className="text-body text-lagado-text-dim mb-2">[VM Display]</p>
          <p className="text-caption text-lagado-text-dim font-mono">
            Width: 60%
          </p>
        </div>
      </div>
 
      {/* Side Pane (40%) */}
      <div className="w-2/5 bg-lagado-bg border-l border-lagado-border flex flex-col">
        {/* Header */}
        <div className="border-b border-lagado-border p-4 flex items-center justify-between">
          <h2 className="text-h3 text-lagado-text-bright font-bold">Side Pane</h2>
          <button className="text-lagado-text-dim hover:text-lagado-text-bright text-xl">
            ✕
          </button>
        </div>
 
        {/* Tabs */}
        <div className="flex border-b border-lagado-border overflow-x-auto">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`
                px-4 py-3 text-body-sm whitespace-nowrap transition-colors
                ${
                  activeTab === tab.id
                    ? "text-lagado-text-bright border-b-2 border-lagado-red"
                    : "text-lagado-text-dim hover:text-lagado-text"
                }
              `}
            >
              {tab.label}
            </button>
          ))}
        </div>
 
        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {activeTab === "reasoning" && (
            <div className="space-y-3">
              <div className="bg-lagado-surface border border-lagado-border rounded-sm p-3">
                <p className="text-caption text-lagado-text-dim mb-1">Step 1</p>
                <p className="text-body-sm text-lagado-text">
                  Understanding the goal: Open Firefox and search for "hello world"
                </p>
              </div>
              <div className="bg-lagado-surface border border-lagado-border rounded-sm p-3">
                <p className="text-caption text-lagado-text-dim mb-1">Step 2</p>
                <p className="text-body-sm text-lagado-text">
                  Plan: Locate Firefox icon in dock, click to launch
                </p>
              </div>
              <div className="bg-lagado-surface border border-lagado-border rounded-sm p-3">
                <p className="text-caption text-lagado-text-dim mb-1">Current Action</p>
                <p className="text-body-sm text-lagado-red font-semibold">
                  → Clicking Firefox icon at (25, 100)
                </p>
              </div>
            </div>
          )}
 
          {activeTab === "tools" && (
            <div className="space-y-2">
              <div className="font-mono text-body-sm text-lagado-text">
                Tools used so far:
              </div>
              <div className="space-y-2 mt-3">
                <div className="flex items-center gap-2 text-body-sm font-mono">
                  <span className="text-lagado-green">✓</span>
                  <span className="text-lagado-text">find_element('firefox')</span>
                </div>
                <div className="flex items-center gap-2 text-body-sm font-mono">
                  <span className="text-lagado-green">✓</span>
                  <span className="text-lagado-text">click(25, 100)</span>
                </div>
                <div className="flex items-center gap-2 text-body-sm font-mono">
                  <span className="text-lagado-yellow animate-pulse">⋯</span>
                  <span className="text-lagado-text">wait_for_window()</span>
                </div>
              </div>
            </div>
          )}
 
          {activeTab === "history" && (
            <div className="space-y-2">
              <p className="text-body-sm text-lagado-text-dim">
                Recent commands:
              </p>
              <div className="space-y-1 mt-2">
                <p className="text-body-sm text-lagado-text">
                  10:00 - Started session
                </p>
                <p className="text-body-sm text-lagado-text">
                  10:01 - Goal: Open Firefox
                </p>
                <p className="text-body-sm text-lagado-text">
                  10:02 - Clicked Firefox icon
                </p>
              </div>
            </div>
          )}
 
          {activeTab === "permissions" && (
            <div className="space-y-3">
              <div>
                <p className="text-h3 text-lagado-text-bright font-semibold mb-2">
                  Currently Approved
                </p>
                <div className="space-y-2">
                  <label className="flex items-center gap-2">
                    <input type="checkbox" defaultChecked className="accent-lagado-red" />
                    <span className="text-body-sm">Documents/</span>
                  </label>
                  <label className="flex items-center gap-2">
                    <input type="checkbox" defaultChecked className="accent-lagado-red" />
                    <span className="text-body-sm">Firefox</span>
                  </label>
                </div>
              </div>
              <Button variant="secondary" size="sm" className="w-full">
                Request New Access
              </Button>
            </div>
          )}
 
          {activeTab === "settings" && (
            <div className="space-y-3">
              <div className="space-y-2">
                <label className="flex items-center justify-between">
                  <span className="text-body-sm">Auto-pause on errors</span>
                  <input type="checkbox" defaultChecked className="accent-lagado-red" />
                </label>
                <label className="flex items-center justify-between">
                  <span className="text-body-sm">Show debug info</span>
                  <input type="checkbox" className="accent-lagado-red" />
                </label>
              </div>
            </div>
          )}
        </div>
 
        {/* Footer */}
        <div className="border-t border-lagado-border p-4 flex gap-2">
          <Button variant="secondary" size="sm" className="flex-1">
            Resume
          </Button>
          <Button variant="primary" size="sm" className="flex-1">
            Stop Agent
          </Button>
        </div>
      </div>
    </div>
  );
}
