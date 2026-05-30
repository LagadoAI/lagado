 
import React, { useState, useRef, useEffect } from "react";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
 
interface TerminalLine {
  type: "command" | "output";
  content: string;
}
 
export default function TerminalDefault() {
  const [lines, setLines] = useState<TerminalLine[]>([
    { type: "command", content: "user@laputa:~$ ls" },
    { type: "output", content: "Desktop  Documents  Downloads  Pictures" },
    { type: "command", content: "user@laputa:~$ pwd" },
    { type: "output", content: "/home/user" },
  ]);
  const [currentInput, setCurrentInput] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const terminalEndRef = useRef<HTMLDivElement>(null);
 
  useEffect(() => {
    terminalEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [lines]);
 
  useEffect(() => {
    inputRef.current?.focus();
  }, []);
 
  const handleCommand = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && currentInput.trim()) {
      setLines((prev) => [
        ...prev,
        { type: "command", content: `user@laputa:~$ ${currentInput}` },
        { type: "output", content: `Command executed: ${currentInput}` },
      ]);
      setCurrentInput("");
    }
  };
 
  return (
    <div className="min-h-screen bg-laputa-bg flex flex-col">
      <Header title="Terminal" />
 
      <div className="flex-1 flex flex-col bg-black">
        {/* Tab bar */}
        <div className="flex border-b border-laputa-border bg-laputa-surface">
          <div className="px-4 py-2 border-r border-laputa-border bg-laputa-surface-2 text-body-sm font-mono">
            <span className="text-laputa-red mr-2">●</span>
            user@laputa
          </div>
          <button className="px-4 py-2 text-laputa-text-dim hover:text-laputa-text">
            +
          </button>
        </div>
 
        {/* Terminal area */}
        <div
          className="flex-1 p-4 overflow-y-auto font-mono text-sm cursor-text"
          onClick={() => inputRef.current?.focus()}
        >
          {lines.map((line, idx) => (
            <div
              key={idx}
              className={
                line.type === "command"
                  ? "text-laputa-green"
                  : "text-laputa-text"
              }
            >
              {line.content}
            </div>
          ))}
          <div className="text-laputa-green flex items-center">
            user@laputa:~${" "}
            <input
              ref={inputRef}
              type="text"
              value={currentInput}
              onChange={(e) => setCurrentInput(e.target.value)}
              onKeyDown={handleCommand}
              className="flex-1 bg-transparent text-laputa-text outline-none ml-1"
              spellCheck={false}
            />
            <span className="w-2 h-4 bg-laputa-red animate-pulse ml-1"></span>
          </div>
          <div ref={terminalEndRef} />
        </div>
 
        {/* Bottom toolbar */}
        <div className="p-3 border-t border-laputa-border bg-laputa-surface flex gap-2">
          <Button variant="secondary" size="sm" onClick={() => setLines([])}>
            Clear
          </Button>
          <Button variant="secondary" size="sm">
            Copy
          </Button>
          <Button variant="secondary" size="sm">
            Save Output
          </Button>
          <Button variant="secondary" size="sm">
            Search
          </Button>
        </div>
      </div>
    </div>
  );
}
