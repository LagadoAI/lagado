 
import React, { useState } from "react";
import { Header } from "../components/Header";
 
export default function TerminalMultipleTabs() {
  const [activeTab, setActiveTab] = useState("term1");
  const tabs = [
    { id: "term1", name: "user@host" },
    { id: "term2", name: "agent_term" },
    { id: "term3", name: "build_log" },
  ];
 
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Terminal - Multi-tab" />
 
      <div className="flex-1 flex flex-col bg-black">
        {/* Tab bar */}
        <div className="flex border-b border-lagado-border bg-lagado-surface">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`px-4 py-2 text-body-sm font-mono flex items-center gap-2 transition-colors ${
                activeTab === tab.id
                  ? "bg-lagado-surface-2 text-lagado-text-bright"
                  : "text-lagado-text-dim hover:text-lagado-text border-r border-lagado-border"
              }`}
            >
              {activeTab === tab.id && <span className="text-lagado-red">●</span>}
              {tab.name}
              <span className="text-lagado-text-dim hover:text-lagado-text ml-2">×</span>
            </button>
          ))}
          <button className="px-4 py-2 text-lagado-text-dim hover:text-lagado-text">
            +
          </button>
        </div>
 
        {/* Terminal content */}
        <div className="flex-1 p-4 font-mono text-sm overflow-y-auto">
          <div className="text-lagado-green">user@lagado:~$ echo "Current tab: {activeTab}"</div>
          <div className="text-lagado-text">Current tab: {activeTab}</div>
          <div className="text-lagado-green flex items-center">
            user@lagado:~${" "}
            <span className="w-2 h-4 bg-lagado-red animate-pulse ml-1"></span>
          </div>
        </div>
      </div>
    </div>
  );
}
