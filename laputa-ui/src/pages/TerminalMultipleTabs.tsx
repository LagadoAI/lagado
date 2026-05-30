 
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
    <div className="min-h-screen bg-laputa-bg flex flex-col">
      <Header title="Terminal - Multi-tab" />
 
      <div className="flex-1 flex flex-col bg-black">
        {/* Tab bar */}
        <div className="flex border-b border-laputa-border bg-laputa-surface">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`px-4 py-2 text-body-sm font-mono flex items-center gap-2 transition-colors ${
                activeTab === tab.id
                  ? "bg-laputa-surface-2 text-laputa-text-bright"
                  : "text-laputa-text-dim hover:text-laputa-text border-r border-laputa-border"
              }`}
            >
              {activeTab === tab.id && <span className="text-laputa-red">●</span>}
              {tab.name}
              <span className="text-laputa-text-dim hover:text-laputa-text ml-2">×</span>
            </button>
          ))}
          <button className="px-4 py-2 text-laputa-text-dim hover:text-laputa-text">
            +
          </button>
        </div>
 
        {/* Terminal content */}
        <div className="flex-1 p-4 font-mono text-sm overflow-y-auto">
          <div className="text-laputa-green">user@laputa:~$ echo "Current tab: {activeTab}"</div>
          <div className="text-laputa-text">Current tab: {activeTab}</div>
          <div className="text-laputa-green flex items-center">
            user@laputa:~${" "}
            <span className="w-2 h-4 bg-laputa-red animate-pulse ml-1"></span>
          </div>
        </div>
      </div>
    </div>
  );
}
