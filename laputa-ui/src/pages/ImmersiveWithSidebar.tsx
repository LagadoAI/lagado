 
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
          <p className="text-body text-laputa-text-dim mb-2">[VM Display]</p>
          <p className="text-caption text-laputa-text-dim font-mono">
            Width: 60%
          </p>
        </div>
      </div>
 
      {/* Side Pane (40%) */}
      <div className="w-2/5 bg-laputa-bg border-l border-laputa-border flex flex-col">
        {/* Header */}
        <div className="border-b border-laputa-border p-4 flex items-center justify-between">
          <h2 className="text-h3 text-laputa-text-bright font-bold">Side Pane</h2>
          <button className="text-laputa-text-dim hover:text-laputa-text-bright text-xl">
            ✕
          </button>
        </div>
 
        {/* Tabs */}
        <div className="flex border-b border-laputa-border overflow-x-auto">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`
                px-4 py-3 text-body-sm whitespace-nowrap transition-colors
                ${
                  activeTab === tab.id
                    ? "text-laputa-text-bright border-b-2 border-laputa-red"
                    : "text-laputa-text-dim hover:text-laputa-text"
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
              <div className="bg-laputa-surface border border-laputa-border rounded-sm p-3">
                <p className="text-caption text-laputa-text-dim mb-1">Step 1</p>
                <p className="text-body-sm text-laputa-text">
                  Understanding the goal: Open Firefox and search for "hello world"
                </p>
              </div>
              <div className="bg-laputa-surface border border-laputa-border rounded-sm p-3">
                <p className="text-caption text-laputa-text-dim mb-1">Step 2</p>
                <p className="text-body-sm text-laputa-text">
                  Plan: Locate Firefox icon in dock, click to launch
                </p>
              </div>
              <div className="bg-laputa-surface border border-laputa-border rounded-sm p-3">
                <p className="text-caption text-laputa-text-dim mb-1">Current Action</p>
                <p className="text-body-sm text-laputa-red font-semibold">
                  → Clicking Firefox icon at (25, 100)
                </p>
              </div>
            </div>
          )}
 
          {activeTab === "tools" && (
            <div className="space-y-2">
              <div className="font-mono text-body-sm text-laputa-text">
                Tools used so far:
              </div>
              <div className="space-y-2 mt-3">
                <div className="flex items-center gap-2 text-body-sm font-mono">
                  <span className="text-laputa-green">✓</span>
                  <span className="text-laputa-text">find_element('firefox')</span>
                </div>
                <div className="flex items-center gap-2 text-body-sm font-mono">
                  <span className="text-laputa-green">✓</span>
                  <span className="text-laputa-text">click(25, 100)</span>
                </div>
                <div className="flex items-center gap-2 text-body-sm font-mono">
                  <span className="text-laputa-yellow animate-pulse">⋯</span>
                  <span className="text-laputa-text">wait_for_window()</span>
                </div>
              </div>
            </div>
          )}
 
          {activeTab === "history" && (
            <div className="space-y-2">
              <p className="text-body-sm text-laputa-text-dim">
                Recent commands:
              </p>
              <div className="space-y-1 mt-2">
                <p className="text-body-sm text-laputa-text">
                  10:00 - Started session
                </p>
                <p className="text-body-sm text-laputa-text">
                  10:01 - Goal: Open Firefox
                </p>
                <p className="text-body-sm text-laputa-text">
                  10:02 - Clicked Firefox icon
                </p>
              </div>
            </div>
          )}
 
          {activeTab === "permissions" && (
            <div className="space-y-3">
              <div>
                <p className="text-h3 text-laputa-text-bright font-semibold mb-2">
                  Currently Approved
                </p>
                <div className="space-y-2">
                  <label className="flex items-center gap-2">
                    <input type="checkbox" defaultChecked className="accent-laputa-red" />
                    <span className="text-body-sm">Documents/</span>
                  </label>
                  <label className="flex items-center gap-2">
                    <input type="checkbox" defaultChecked className="accent-laputa-red" />
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
                  <input type="checkbox" defaultChecked className="accent-laputa-red" />
                </label>
                <label className="flex items-center justify-between">
                  <span className="text-body-sm">Show debug info</span>
                  <input type="checkbox" className="accent-laputa-red" />
                </label>
              </div>
            </div>
          )}
        </div>
 
        {/* Footer */}
        <div className="border-t border-laputa-border p-4 flex gap-2">
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
