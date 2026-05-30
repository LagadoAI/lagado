import React, { useState, useRef, useEffect } from "react";
import { useNavigate } from "react-router-dom";

interface Message {
  id: string;
  role: "user" | "agent";
  content: string;
  thinkingSummary?: string;
  thinkingFull?: string;
  timestamp: Date;
  isTyping?: boolean;
}

interface Conversation {
  id: string;
  title: string;
  updatedAt: Date;
}

const DETECTED_MODELS: { id: string; name: string; status: "connected" | "available" }[] = [];
const PROJECTS: { id: string; name: string }[] = [];

export default function ChatDefault() {
  const navigate = useNavigate();
  const [messages, setMessages] = useState<Message[]>([
    { id: "welcome", role: "agent", content: "Hi! How can I help you today?", timestamp: new Date() },
  ]);
  const [input, setInput] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [expandedThinking, setExpandedThinking] = useState<Set<string>>(new Set());
  const [showSidebar, setShowSidebar] = useState(true);
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [showPlusMenu, setShowPlusMenu] = useState(false);
  const [agentRunning, setAgentRunning] = useState(false);
  const [agentPaused, setAgentPaused] = useState(false);
  const [conversations] = useState<Conversation[]>([]);
  const [searchTerm, setSearchTerm] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const selectedModel = DETECTED_MODELS[0];
  const isConnected = !!selectedModel;

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const formatTimestamp = (d: Date) => {
    const today = new Date();
    const isToday = d.toDateString() === today.toDateString();
    const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return isToday ? time : `${d.toLocaleDateString()} ${time}`;
  };

  const handleSend = () => {
    if (!input.trim()) return;
    const userMsg: Message = { id: Date.now().toString(), role: "user", content: input, timestamp: new Date() };
    setMessages((prev) => [...prev, userMsg]);
    setInput("");

    const typingMsg: Message = { id: "typing-" + Date.now(), role: "agent", content: "", timestamp: new Date(), isTyping: true };
    setMessages((prev) => [...prev, typingMsg]);

    setTimeout(() => {
      setMessages((prev) => prev.filter((m) => !m.isTyping));
      if (!isConnected) {
        setMessages((prev) => [...prev, {
          id: "err-" + Date.now(), role: "agent",
          content: "No model connected. Start your llama-server or connect a model in Server settings.",
          timestamp: new Date(),
        }]);
      } else {
        setMessages((prev) => [...prev, {
          id: "resp-" + Date.now(), role: "agent",
          content: `Working on: "${userMsg.content}"`,
          thinkingSummary: "Parsing intent and selecting tools...",
          thinkingFull: `1. Parse intent from: "${userMsg.content}"\n2. Identify required tools\n3. Build action plan\n4. Execute and verify`,
          timestamp: new Date(),
        }]);
      }
    }, 1200);
  };

  const startEdit = (msg: Message) => { setEditingId(msg.id); setEditValue(msg.content); };
  const saveEdit = () => {
    if (!editingId) return;
    setMessages((prev) => {
      const idx = prev.findIndex((m) => m.id === editingId);
      if (idx === -1) return prev;
      const updated = [...prev.slice(0, idx + 1)];
      updated[idx] = { ...updated[idx], content: editValue };
      return updated;
    });
    setEditingId(null); setEditValue("");
  };
  const toggleThinking = (id: string) => {
    setExpandedThinking((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };
  const copyMessage = (content: string) => navigator.clipboard.writeText(content);
  const togglePauseAgent = () => { if (agentRunning) setAgentPaused(!agentPaused); };

  const filteredConvs = conversations.filter((c) =>
    c.title.toLowerCase().includes(searchTerm.toLowerCase())
  );

  return (
    <div className="h-screen bg-laputa-bg flex overflow-hidden">
      {/* SIDEBAR */}
      {showSidebar && (
        <div className="w-64 bg-laputa-surface flex flex-col">
          {/* New Chat */}
          <div className="p-3">
            <button
              onClick={() => setMessages([{ id: "welcome", role: "agent", content: "Hi! How can I help you today?", timestamp: new Date() }])}
              className="w-full px-3 py-2 bg-laputa-red text-white rounded-md text-body-sm font-semibold hover:bg-opacity-90 transition-colors"
            >
              + New conversation
            </button>
          </div>

          {/* Search */}
          <div className="px-3 pb-3">
            <input
              type="text"
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              placeholder="Search chats..."
              className="w-full px-3 py-1.5 bg-laputa-surface-2 border border-laputa-border rounded-md text-body-sm text-laputa-text placeholder-laputa-text-dim focus:border-laputa-red focus:outline-none"
            />
          </div>

          {/* Projects (above Recent) */}
          <div className="flex-1 overflow-y-auto">
            <div className="px-3 py-2 text-caption text-laputa-text-dim font-semibold uppercase tracking-wider">
              Projects
            </div>
            {PROJECTS.length === 0 ? (
              <button className="w-full text-left px-3 py-2 mx-2 my-1 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text-dim">
                + New project
              </button>
            ) : (
              PROJECTS.map((p) => (
                <div key={p.id} className="px-3 py-2 mx-2 my-1 rounded-md hover:bg-laputa-surface-2 cursor-pointer text-body-sm text-laputa-text">
                  {p.name}
                </div>
              ))
            )}

            {/* Recent */}
            <div className="px-3 py-2 mt-2 text-caption text-laputa-text-dim font-semibold uppercase tracking-wider">
              Recent
            </div>
            {filteredConvs.length === 0 ? (
              <div className="px-3 py-2 mx-2 text-body-sm text-laputa-text-dim italic">
                No conversations yet
              </div>
            ) : (
              filteredConvs.map((conv) => (
                <div key={conv.id} className="px-3 py-2 mx-2 my-1 rounded-md hover:bg-laputa-surface-2 cursor-pointer text-body-sm text-laputa-text truncate">
                  {conv.title}
                </div>
              ))
            )}
          </div>

          {/* Navigation - LINE STAYS HERE */}
          <div className="border-t border-laputa-border p-2 space-y-1">
            <button onClick={() => navigate("/immersive")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              Immersive
            </button>
            <button onClick={() => navigate("/code")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              Code
            </button>
            <button onClick={() => navigate("/vault")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              Vault
            </button>
            <button onClick={() => navigate("/terminal")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              Terminal
            </button>
            <button onClick={() => navigate("/mcp")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              MCP
            </button>
            <button onClick={() => navigate("/server")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              Server
            </button>
            <button onClick={() => navigate("/vm")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              VM Manager
            </button>
            <button onClick={() => navigate("/settings")} className="w-full text-left px-3 py-2 rounded-md hover:bg-laputa-surface-2 text-body-sm text-laputa-text">
              Settings
            </button>
          </div>
        </div>
      )}

      {/* MAIN CHAT */}
      <div className="flex-1 flex flex-col">
        {/* Top bar */}
        <div className="border-b border-laputa-border bg-laputa-surface px-4 py-3 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => setShowSidebar(!showSidebar)} className="text-laputa-text hover:text-laputa-text-bright text-xl">
              ≡
            </button>
            <h1 className="text-h3 text-laputa-text-bright font-semibold">Chat</h1>
          </div>

          <div className="flex items-center gap-3">
            {agentRunning && (
              <button
                onClick={togglePauseAgent}
                className="px-3 py-1 rounded-md bg-laputa-surface-2 border border-laputa-border hover:border-laputa-red text-body-sm text-laputa-text transition-colors"
                title={agentPaused ? "Resume agent" : "Pause agent"}
              >
                {agentPaused ? "Resume" : "Pause"}
              </button>
            )}

            <div className="flex items-center gap-2">
              <span className={`w-2 h-2 rounded-full ${isConnected ? "bg-laputa-green" : "bg-laputa-red"}`} />
              <span className="text-body-sm text-laputa-text-dim">
                {isConnected ? `Connected: ${selectedModel.name}` : "No model connected"}
              </span>
            </div>
          </div>
        </div>

        {/* Connection warning */}
        {!isConnected && (
          <div className="bg-laputa-red bg-opacity-10 border-b border-laputa-red px-4 py-2">
            <div className="flex items-center gap-2 max-w-3xl mx-auto">
              <span className="text-laputa-red font-semibold">!</span>
              <span className="text-body-sm text-laputa-text">
                Start llama-server to enable chat.
              </span>
              <button onClick={() => navigate("/server")} className="ml-auto text-body-sm text-laputa-red underline hover:text-opacity-80">
                Configure
              </button>
            </div>
          </div>
        )}

        {/* Messages */}
        <div className="flex-1 overflow-y-auto p-6">
          <div className="max-w-3xl mx-auto space-y-6">
            {messages.map((msg) => (
              <div key={msg.id} className="group">
                {msg.role === "agent" ? (
                  <div className="flex gap-3">
                    {/* Logo placeholder - swap with image when ready */}
                    <div className="w-8 h-8 rounded-md bg-laputa-surface-2 border border-laputa-border flex-shrink-0" />
                    <div className="flex-1">
                      {msg.isTyping ? (
                        <TypingAnimation />
                      ) : (
                        <>
                          {msg.thinkingSummary && (
                            <button
                              onClick={() => toggleThinking(msg.id)}
                              className="mb-2 text-caption text-laputa-text-dim hover:text-laputa-text flex items-center gap-1 italic"
                            >
                              <span>{expandedThinking.has(msg.id) ? "▼" : "▶"}</span>
                              <span>{msg.thinkingSummary}</span>
                            </button>
                          )}
                          {expandedThinking.has(msg.id) && msg.thinkingFull && (
                            <div className="mb-3 p-3 bg-laputa-surface border-l-2 border-laputa-purple rounded-r-md">
                              <pre className="text-caption text-laputa-text-dim whitespace-pre-wrap font-mono">
                                {msg.thinkingFull}
                              </pre>
                            </div>
                          )}

                          <p className="text-body text-laputa-text whitespace-pre-wrap leading-relaxed">
                            {msg.content}
                          </p>

                          {/* Action buttons - text labels, no emojis */}
                          <div className="flex items-center gap-3 mt-2 opacity-0 group-hover:opacity-100 transition-opacity">
                            <button onClick={() => copyMessage(msg.content)} className="text-caption text-laputa-text-dim hover:text-laputa-text">
                              Copy
                            </button>
                            <button className="text-caption text-laputa-text-dim hover:text-laputa-green">
                              Good
                            </button>
                            <button className="text-caption text-laputa-text-dim hover:text-laputa-red">
                              Bad
                            </button>
                            <button className="text-caption text-laputa-text-dim hover:text-laputa-text">
                              Regenerate
                            </button>
                            <span className="text-caption text-laputa-text-dim ml-auto">
                              {formatTimestamp(msg.timestamp)}
                            </span>
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                ) : (
                  <div className="flex justify-end">
                    <div className="max-w-2xl">
                      <div className="bg-laputa-red text-white p-3 rounded-2xl rounded-tr-sm">
                        {editingId === msg.id ? (
                          <div>
                            <textarea
                              value={editValue}
                              onChange={(e) => setEditValue(e.target.value)}
                              className="w-full bg-transparent text-white outline-none resize-none border border-white border-opacity-30 rounded-md p-2"
                              rows={3}
                              autoFocus
                            />
                            <div className="flex gap-2 mt-2 justify-end">
                              <button onClick={() => { setEditingId(null); setEditValue(""); }} className="px-3 py-1 text-body-sm bg-white bg-opacity-10 hover:bg-opacity-20 rounded-md">
                                Cancel
                              </button>
                              <button onClick={saveEdit} className="px-3 py-1 text-body-sm bg-white text-laputa-red rounded-md font-semibold">
                                Save & Resend
                              </button>
                            </div>
                          </div>
                        ) : (
                          <p className="text-body whitespace-pre-wrap">{msg.content}</p>
                        )}
                      </div>
                      <div className="flex items-center justify-end gap-2 mt-1 opacity-0 group-hover:opacity-100 transition-opacity">
                        {editingId !== msg.id && (
                          <button onClick={() => startEdit(msg)} className="text-caption text-laputa-text-dim hover:text-laputa-text">
                            Edit
                          </button>
                        )}
                        <span className="text-caption text-laputa-text-dim">
                          {formatTimestamp(msg.timestamp)}
                        </span>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            ))}
            <div ref={messagesEndRef} />
          </div>
        </div>

        {/* Input */}
        <div className="border-t border-laputa-border bg-laputa-surface p-4">
          <div className="max-w-3xl mx-auto">
            <div className="bg-laputa-surface-2 border border-laputa-border rounded-2xl p-3">
              <textarea
                value={input}
                onChange={(e) => setInput(e.target.value)}
                placeholder={isConnected ? "Type your message..." : "Type message (no model connected)..."}
                rows={2}
                className="w-full bg-transparent text-laputa-text placeholder-laputa-text-dim outline-none resize-none"
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); }
                }}
              />

              <div className="flex items-center justify-between mt-2 pt-2 border-t border-laputa-border">
                <div className="flex items-center gap-2">
                  {/* Plus button */}
                  <div className="relative">
                    <button
                      onClick={() => setShowPlusMenu(!showPlusMenu)}
                      className="w-7 h-7 rounded-full bg-laputa-surface border border-laputa-border hover:border-laputa-red text-laputa-text hover:text-laputa-red flex items-center justify-center transition-colors"
                    >
                      +
                    </button>
                    {showPlusMenu && (
                      <div className="absolute bottom-full mb-2 left-0 bg-laputa-surface border border-laputa-border rounded-md py-1 min-w-[180px] shadow-lg z-10">
                        <button onClick={() => { fileInputRef.current?.click(); setShowPlusMenu(false); }} className="w-full text-left px-3 py-2 text-body-sm text-laputa-text hover:bg-laputa-surface-2">
                          Attach file
                        </button>
                        <button onClick={() => { navigate("/vault"); setShowPlusMenu(false); }} className="w-full text-left px-3 py-2 text-body-sm text-laputa-text hover:bg-laputa-surface-2">
                          From Vault
                        </button>
                        <button onClick={() => setShowPlusMenu(false)} className="w-full text-left px-3 py-2 text-body-sm text-laputa-text hover:bg-laputa-surface-2">
                          Screenshot
                        </button>
                      </div>
                    )}
                    <input
                      ref={fileInputRef}
                      type="file"
                      multiple
                      className="hidden"
                      onChange={(e) => { if (e.target.files) alert(`Attached ${e.target.files.length} file(s)`); }}
                    />
                  </div>

                  {/* Model selector */}
                  <div className="relative">
                    <button
                      onClick={() => setShowModelMenu(!showModelMenu)}
                      className="px-3 py-1 rounded-md bg-laputa-surface border border-laputa-border hover:border-laputa-red text-body-sm text-laputa-text flex items-center gap-2 transition-colors"
                    >
                      <span className={`w-1.5 h-1.5 rounded-full ${isConnected ? "bg-laputa-green" : "bg-laputa-red"}`} />
                      {isConnected ? selectedModel.name : "No model"}
                      <span className="text-laputa-text-dim">▾</span>
                    </button>
                    {showModelMenu && (
                      <div className="absolute bottom-full mb-2 left-0 bg-laputa-surface border border-laputa-border rounded-md py-1 min-w-[200px] shadow-lg z-10">
                        {DETECTED_MODELS.length === 0 ? (
                          <div className="px-3 py-2 text-body-sm text-laputa-text-dim italic">
                            No models detected
                          </div>
                        ) : (
                          DETECTED_MODELS.map((m) => (
                            <button
                              key={m.id}
                              onClick={() => setShowModelMenu(false)}
                              className="w-full text-left px-3 py-2 text-body-sm text-laputa-text hover:bg-laputa-surface-2 flex items-center gap-2"
                            >
                              <span className={`w-1.5 h-1.5 rounded-full ${m.status === "connected" ? "bg-laputa-green" : "bg-laputa-text-dim"}`} />
                              {m.name}
                            </button>
                          ))
                        )}
                        <div className="border-t border-laputa-border mt-1 pt-1">
                          <button
                            onClick={() => { navigate("/server"); setShowModelMenu(false); }}
                            className="w-full text-left px-3 py-2 text-body-sm text-laputa-purple hover:bg-laputa-surface-2"
                          >
                            Manage models...
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                </div>

                <button
                  onClick={handleSend}
                  disabled={!input.trim()}
                  className={`px-4 py-1.5 rounded-md text-body-sm font-semibold transition-colors ${
                    input.trim()
                      ? "bg-laputa-red text-white hover:bg-opacity-90"
                      : "bg-laputa-surface border border-laputa-border text-laputa-text-dim cursor-not-allowed"
                  }`}
                >
                  Send
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function TypingAnimation() {
  const [stage, setStage] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setStage((s) => (s + 1) % 4), 400);
    return () => clearInterval(id);
  }, []);
  return (
    <div className="flex items-center gap-1 text-laputa-text-dim text-body py-1">
      <span>.</span>
      <span className={stage >= 1 ? "opacity-100" : "opacity-0"}>.</span>
      <span className={stage >= 2 ? "opacity-100" : "opacity-0"}>.</span>
    </div>
  );
}
