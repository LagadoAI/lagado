 
import React from "react";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
 
export default function TerminalAgentRunning() {
  return (
    <div className="min-h-screen bg-laputa-bg flex flex-col">
      <Header title="Terminal - Agent Running" />
 
      <div className="flex-1 flex flex-col bg-black">
        {/* Agent indicator banner */}
        <div className="bg-laputa-green bg-opacity-20 border-b border-laputa-green px-4 py-2">
          <div className="flex items-center gap-3">
            <span className="text-laputa-green text-xl animate-pulse">⚡</span>
            <span className="text-body text-laputa-text-bright font-semibold">
              Agent is running commands
            </span>
          </div>
        </div>
 
        {/* Tab bar */}
        <div className="flex border-b border-laputa-border bg-laputa-surface">
          <div className="px-4 py-2 bg-laputa-surface-2 text-body-sm font-mono text-laputa-text-bright">
            <span className="text-laputa-red mr-2">●</span>
            agent_executing
          </div>
        </div>
 
        {/* Terminal content */}
        <div className="flex-1 p-4 font-mono text-sm overflow-y-auto">
          <div className="text-laputa-green">user@laputa:~$ git status</div>
          <div className="text-laputa-text">On branch main</div>
          <div className="text-laputa-text">Your branch is up to date with 'origin/main'.</div>
          <div className="text-laputa-text mt-2">nothing to commit, working tree clean</div>
          <div className="text-laputa-green flex items-center mt-2">
            user@laputa:~${" "}
            <span className="w-2 h-4 bg-laputa-red animate-pulse ml-1"></span>
          </div>
        </div>
 
        {/* Bottom toolbar */}
        <div className="p-3 border-t border-laputa-border bg-laputa-surface flex gap-2">
          <Button variant="primary" size="sm">Pause Agent</Button>
          <Button variant="secondary" size="sm">Take Over</Button>
        </div>
      </div>
    </div>
  );
}
 

