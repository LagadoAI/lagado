// Same as CodePage but with output panel visible
 
export { default as CodeWithSandboxOutput } from "./CodePage";
 
 
import React, { useState } from "react";
import { Header } from "../components/Header";
 
export default function CodeWithTerminal() {
  const [activeTab, setActiveTab] = useState("output");
  const [code] = useState(`def greet(name):
    print(f"Hello, {name}!")
 
greet("World")`);
 
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Code" />
 
      <div className="flex-1 flex flex-col">
        {/* File browser + Editor (split) */}
        <div className="flex-1 flex">
          <div className="w-64 border-r border-lagado-border bg-lagado-surface p-4">
            <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">Files</h3>
            <div className="space-y-1 text-body-sm font-mono">
              <div className="px-2 py-1 hover:bg-lagado-surface-2 rounded cursor-pointer text-lagado-text">📁 src/</div>
              <div className="px-2 py-1 bg-lagado-surface-2 rounded cursor-pointer text-lagado-text-bright">📄 test.py</div>
            </div>
          </div>
 
          <div className="flex-1 flex flex-col">
            <div className="flex-1 p-4 font-mono text-sm">
              <pre className="text-lagado-text">{code}</pre>
            </div>
          </div>
        </div>
 
        {/* Bottom panel with tabs (Output / Terminal) */}
        <div className="h-64 border-t border-lagado-border bg-lagado-surface">
          <div className="flex border-b border-lagado-border">
            <button
              onClick={() => setActiveTab("output")}
              className={`px-4 py-2 text-body-sm font-mono transition-colors ${
                activeTab === "output"
                  ? "text-lagado-text-bright border-b-2 border-lagado-red bg-lagado-surface-2"
                  : "text-lagado-text-dim hover:text-lagado-text"
              }`}
            >
              ▶ Output
            </button>
            <button
              onClick={() => setActiveTab("terminal")}
              className={`px-4 py-2 text-body-sm font-mono transition-colors ${
                activeTab === "terminal"
                  ? "text-lagado-text-bright border-b-2 border-lagado-red bg-lagado-surface-2"
                  : "text-lagado-text-dim hover:text-lagado-text"
              }`}
            >
              ▷ Terminal
            </button>
          </div>
 
          <div className="p-4 h-full font-mono text-sm overflow-y-auto bg-lagado-bg">
            {activeTab === "output" && (
              <>
                <div className="text-lagado-green">$ python test.py</div>
                <div className="text-lagado-text">Hello, World!</div>
                <div className="text-lagado-text-dim mt-2">Process exited (code 0)</div>
              </>
            )}
            {activeTab === "terminal" && (
              <>
                <div className="text-lagado-green">user@lagado:~$ ls</div>
                <div className="text-lagado-text">Desktop  Documents  Downloads</div>
                <div className="text-lagado-green flex items-center">
                  user@lagado:~${" "}
                  <span className="ml-1 w-2 h-4 bg-lagado-red animate-pulse"></span>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

