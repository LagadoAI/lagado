 
import React from "react";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
 
export default function TerminalAgentRunning() {
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Terminal - Agent Running" />
 
      <div className="flex-1 flex flex-col bg-black">
        {/* Agent indicator banner */}
        <div className="bg-lagado-green bg-opacity-20 border-b border-lagado-green px-4 py-2">
          <div className="flex items-center gap-3">
            <span className="text-lagado-green text-xl animate-pulse">⚡</span>
            <span className="text-body text-lagado-text-bright font-semibold">
              Agent is running commands
            </span>
          </div>
        </div>
 
        {/* Tab bar */}
        <div className="flex border-b border-lagado-border bg-lagado-surface">
          <div className="px-4 py-2 bg-lagado-surface-2 text-body-sm font-mono text-lagado-text-bright">
            <span className="text-lagado-red mr-2">●</span>
            agent_executing
          </div>
        </div>
 
        {/* Terminal content */}
        <div className="flex-1 p-4 font-mono text-sm overflow-y-auto">
          <div className="text-lagado-green">user@lagado:~$ git status</div>
          <div className="text-lagado-text">On branch main</div>
          <div className="text-lagado-text">Your branch is up to date with 'origin/main'.</div>
          <div className="text-lagado-text mt-2">nothing to commit, working tree clean</div>
          <div className="text-lagado-green flex items-center mt-2">
            user@lagado:~${" "}
            <span className="w-2 h-4 bg-lagado-red animate-pulse ml-1"></span>
          </div>
        </div>
 
        {/* Bottom toolbar */}
        <div className="p-3 border-t border-lagado-border bg-lagado-surface flex gap-2">
          <Button variant="primary" size="sm">Pause Agent</Button>
          <Button variant="secondary" size="sm">Take Over</Button>
        </div>
      </div>
    </div>
  );
}
 

