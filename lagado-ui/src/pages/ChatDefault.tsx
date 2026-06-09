import React, { useState, useRef, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { useChatContext } from "@/hooks/use-chat-context";
import { PermissionCard } from "@/components/PermissionCard";

interface Conversation {
  id: string;
  title: string;
  updatedAt: Date;
}

const PROJECTS: { id: string; name: string }[] = [];

export default function ChatDefault() {
  const navigate = useNavigate();
  const { messages, sendMessage, status, isPaused, setIsPaused, connState, pendingPermission, approve, deny } = useChatContext();

  const surfaceRoutes: Record<string, string> = {
    immersive: "/immersive",
    chat: "/chat",
    code: "/code",
  };

  const [input, setInput] = useState("");
  const [showSidebar, setShowSidebar] = useState(true);
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [showPlusMenu, setShowPlusMenu] = useState(false);
  const [conversations] = useState<Conversation[]>([]);
  const [searchTerm, setSearchTerm] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const isConnected = connState === "connected";
  const isLoading = status === "submitted" || status === "streaming";

useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = () => {
    if (!input.trim() || isLoading) return;
    sendMessage({ text: input });
    setInput("");
  };

  const copyMessage = (content: string) => navigator.clipboard.writeText(content);

  const filteredConvs = conversations.filter((c) =>
    c.title.toLowerCase().includes(searchTerm.toLowerCase())
  );

  const connLabel =
    connState === "connected" ? "Connected"
    : connState === "connecting" ? "Connecting..."
    : "Disconnected";

  const connDot =
    connState === "connected"   ? "bg-lagado-green shadow-[0_0_8px_rgba(34,197,94,0.5)]"
  : connState === "connecting"  ? "bg-lagado-yellow animate-pulse"
  : "bg-lagado-red";

  return (
    <div className="h-screen bg-lagado-bg flex overflow-hidden">
      {/* SIDEBAR */}
      {showSidebar && (
        <div className="w-64 bg-lagado-surface/40 backdrop-blur-md border-r border-lagado-border/50 flex flex-col">
          <div className="p-3">
            <button className="w-full px-3 py-2 bg-gradient-to-r from-lagado-blue to-lagado-purple text-white rounded-md text-body-sm font-semibold hover:opacity-90 hover:shadow-[0_0_15px_rgba(139,92,246,0.3)] transition-all">
              + New conversation
            </button>
          </div>

          <div className="px-3 pb-3">
            <input
              type="text"
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              placeholder="Search chats..."
              className="w-full px-3 py-1.5 bg-lagado-surface-2 border border-lagado-border rounded-md text-body-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-red focus:outline-none"
            />
          </div>

          <div className="flex-1 overflow-y-auto">
            <div className="px-3 py-2 text-caption text-lagado-text-dim font-semibold uppercase tracking-wider">
              Projects
            </div>
            {PROJECTS.length === 0 ? (
              <button className="w-full text-left px-3 py-2 mx-2 my-1 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text-dim">
                + New project
              </button>
            ) : (
              PROJECTS.map((p) => (
                <div key={p.id} className="px-3 py-2 mx-2 my-1 rounded-md hover:bg-lagado-surface-2 cursor-pointer text-body-sm text-lagado-text">
                  {p.name}
                </div>
              ))
            )}

            <div className="px-3 py-2 mt-2 text-caption text-lagado-text-dim font-semibold uppercase tracking-wider">
              Recent
            </div>
            {filteredConvs.length === 0 ? (
              <div className="px-3 py-2 mx-2 text-body-sm text-lagado-text-dim italic">
                No conversations yet
              </div>
            ) : (
              filteredConvs.map((conv) => (
                <div key={conv.id} className="px-3 py-2 mx-2 my-1 rounded-md hover:bg-lagado-surface-2 cursor-pointer text-body-sm text-lagado-text truncate">
                  {conv.title}
                </div>
              ))
            )}
          </div>

          <div className="border-t border-lagado-border p-2 space-y-1">
            <button onClick={() => navigate("/immersive")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              Immersive
            </button>
            <button onClick={() => navigate("/code")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              Code
            </button>
            <button onClick={() => navigate("/vault")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              Vault
            </button>
            <button onClick={() => navigate("/terminal")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              Terminal
            </button>
            <button onClick={() => navigate("/mcp")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              MCP
            </button>
            <button onClick={() => navigate("/server")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              Server
            </button>
            <button onClick={() => navigate("/vm")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              VM Manager
            </button>
            <button onClick={() => navigate("/settings")} className="w-full text-left px-3 py-2 rounded-md hover:bg-lagado-surface-2 text-body-sm text-lagado-text">
              Settings
            </button>
          </div>
        </div>
      )}

      {/* MAIN CHAT */}
      <div className="flex-1 flex flex-col">
        {/* Top bar */}
        <div className="border-b border-lagado-border/50 bg-lagado-surface/60 backdrop-blur-md px-4 py-3 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => setShowSidebar(!showSidebar)} className="text-lagado-text hover:text-lagado-text-bright text-xl">
              ≡
            </button>
            <h1 className="text-h3 font-semibold bg-gradient-to-r from-lagado-blue to-lagado-purple bg-clip-text text-transparent">Chat</h1>
          </div>

          <div className="flex items-center gap-3">
            {isLoading && (
              <button
                onClick={() => setIsPaused(!isPaused)}
                className="px-3 py-1 rounded-md bg-lagado-surface-2 border border-lagado-border hover:border-lagado-blue hover:shadow-[0_0_10px_rgba(59,130,246,0.2)] text-body-sm text-lagado-text transition-all"
              >
                {isPaused ? "Resume" : "Pause"}
              </button>
            )}

            <div className="flex items-center gap-2">
              <span className={`w-2 h-2 rounded-full ${connDot}`} />
              <span className="text-body-sm text-lagado-text-dim">{connLabel}</span>
            </div>
          </div>
        </div>

        {/* Connection warning */}
        {!isConnected && (
          <div className="bg-lagado-red bg-opacity-10 border-b border-lagado-red px-4 py-2">
            <div className="flex items-center gap-2 max-w-3xl mx-auto">
              <span className="text-lagado-red font-semibold">!</span>
              <span className="text-body-sm text-lagado-text">
                {connState === "connecting"
                  ? "Connecting to agent on :9090..."
                  : "Agent disconnected. Start lagado-agent to enable chat."}
              </span>
              <button onClick={() => navigate("/server")} className="ml-auto text-body-sm text-lagado-red underline hover:text-opacity-80">
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
                {msg.role === "assistant" ? (
                  <div className="flex gap-3">
                    <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-lagado-blue to-lagado-purple flex-shrink-0 flex items-center justify-center shadow-[0_0_12px_rgba(139,92,246,0.3)]">
                      <span className="text-white text-caption font-bold">L</span>
                    </div>
                    <div className="flex-1 max-w-2xl">
                      <div className="relative bg-lagado-surface/60 backdrop-blur-md border border-lagado-border/60 rounded-2xl rounded-tl-sm p-4 shadow-lg">
                        <div className="absolute left-0 top-4 bottom-4 w-0.5 rounded-full bg-gradient-to-b from-lagado-blue to-lagado-purple" />
                        <p className="pl-4 text-body text-lagado-text whitespace-pre-wrap leading-relaxed">
                          {msg.content}
                        </p>
                      </div>
                      <div className="flex items-center gap-3 mt-2 pl-4 opacity-0 group-hover:opacity-100 transition-opacity">
                        <button onClick={() => copyMessage(msg.content)} className="text-caption text-lagado-text-dim hover:text-lagado-text">
                          Copy
                        </button>
                        <button className="text-caption text-lagado-text-dim hover:text-lagado-green">
                          Good
                        </button>
                        <button className="text-caption text-lagado-text-dim hover:text-lagado-red">
                          Bad
                        </button>
                      </div>
                    </div>
                  </div>
                ) : (
                  <div className="flex justify-end">
                    <div className="max-w-2xl">
                      <div className="bg-gradient-to-br from-lagado-blue to-lagado-purple text-white p-4 rounded-2xl rounded-tr-sm shadow-lg shadow-lagado-blue/20">
                        <p className="text-body whitespace-pre-wrap leading-relaxed">{msg.content}</p>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            ))}
            {isLoading && (
              <div className="flex gap-3">
                <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-lagado-blue to-lagado-purple flex-shrink-0 shadow-[0_0_12px_rgba(139,92,246,0.3)]" />
                <div className="bg-lagado-surface/60 backdrop-blur-md border border-lagado-border/60 rounded-2xl rounded-tl-sm px-4 py-3">
                  <TypingAnimation />
                </div>
              </div>
            )}
            {pendingPermission && (
              <PermissionCard
                req={pendingPermission}
                onApprove={() => approve(pendingPermission.id)}
                onDeny={() => deny(pendingPermission.id)}
                onSwitch={(surface) => navigate(surfaceRoutes[surface] ?? "/chat")}
              />
            )}
            <div ref={messagesEndRef} />
          </div>
        </div>

        {/* Input */}
        <div className="border-t border-lagado-border bg-lagado-surface p-4">
          <div className="max-w-3xl mx-auto">
            <div className="bg-lagado-surface/60 backdrop-blur-md border border-lagado-border/60 rounded-2xl p-3 focus-within:border-lagado-blue/60 focus-within:shadow-[0_0_20px_rgba(59,130,246,0.12)] transition-all duration-200">
              <textarea
                value={input}
                onChange={(e) => setInput(e.target.value)}
                placeholder={isConnected ? "Type your message..." : "Type message (agent not connected)..."}
                rows={2}
                className="w-full bg-transparent text-lagado-text placeholder-lagado-text-dim outline-none resize-none"
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); }
                }}
              />

              <div className="flex items-center justify-between mt-2 pt-2 border-t border-lagado-border">
                <div className="flex items-center gap-2">
                  <div className="relative">
                    <button
                      onClick={() => setShowPlusMenu(!showPlusMenu)}
                      className="w-7 h-7 rounded-full bg-lagado-surface border border-lagado-border hover:border-lagado-blue hover:text-lagado-blue flex items-center justify-center transition-colors"
                    >
                      +
                    </button>
                    {showPlusMenu && (
                      <div className="absolute bottom-full mb-2 left-0 bg-lagado-surface border border-lagado-border rounded-md py-1 min-w-[180px] shadow-lg z-10">
                        <button onClick={() => { fileInputRef.current?.click(); setShowPlusMenu(false); }} className="w-full text-left px-3 py-2 text-body-sm text-lagado-text hover:bg-lagado-surface-2">
                          Attach file
                        </button>
                        <button onClick={() => { navigate("/vault"); setShowPlusMenu(false); }} className="w-full text-left px-3 py-2 text-body-sm text-lagado-text hover:bg-lagado-surface-2">
                          From Vault
                        </button>
                        <button onClick={() => setShowPlusMenu(false)} className="w-full text-left px-3 py-2 text-body-sm text-lagado-text hover:bg-lagado-surface-2">
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

                  <div className="relative">
                    <button
                      onClick={() => setShowModelMenu(!showModelMenu)}
                      className="px-3 py-1 rounded-md bg-lagado-surface border border-lagado-border hover:border-lagado-red text-body-sm text-lagado-text flex items-center gap-2 transition-colors"
                    >
                      <span className={`w-1.5 h-1.5 rounded-full ${connDot}`} />
                      {connLabel}
                      <span className="text-lagado-text-dim">▾</span>
                    </button>
                    {showModelMenu && (
                      <div className="absolute bottom-full mb-2 left-0 bg-lagado-surface border border-lagado-border rounded-md py-1 min-w-[200px] shadow-lg z-10">
                        <div className="border-t border-lagado-border mt-1 pt-1">
                          <button
                            onClick={() => { navigate("/server"); setShowModelMenu(false); }}
                            className="w-full text-left px-3 py-2 text-body-sm text-lagado-purple hover:bg-lagado-surface-2"
                          >
                            Manage server...
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                </div>

                <button
                  onClick={handleSend}
                  disabled={!input.trim() || isLoading}
                  className={`px-4 py-1.5 rounded-md text-body-sm font-semibold transition-all ${
                    input.trim() && !isLoading
                      ? "bg-gradient-to-r from-lagado-blue to-lagado-purple text-white hover:opacity-90 hover:shadow-[0_0_12px_rgba(139,92,246,0.3)]"
                      : "bg-lagado-surface border border-lagado-border text-lagado-text-dim cursor-not-allowed"
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
    <div className="flex items-center gap-1 text-lagado-text-dim text-body py-1">
      <span>.</span>
      <span className={stage >= 1 ? "opacity-100" : "opacity-0"}>.</span>
      <span className={stage >= 2 ? "opacity-100" : "opacity-0"}>.</span>
    </div>
  );
}
