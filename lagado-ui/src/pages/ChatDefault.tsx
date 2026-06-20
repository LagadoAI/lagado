import React, { useState, useRef, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { useChatContext } from "@/hooks/use-chat-context";
import { PermissionCard } from "@/components/PermissionCard";
import { HyperLoader } from "../components/HyperLoader";
import { AppSidebar } from "../components/AppSidebar";
import { PanelLeft, Paperclip, Mic, ArrowUp, Square } from "lucide-react";

const surfaceRoutes: Record<string, string> = {
  immersive: "/immersive",
  chat: "/chat",
  code: "/code",
};

export default function ChatDefault() {
  const navigate = useNavigate();
  const { messages, sendMessage, status, isPaused, setIsPaused, connState, pendingPermission, approve, deny, stop } = useChatContext();

  const [input, setInput] = useState("");
  const [showSidebar, setShowSidebar] = useState(true);
  const messagesEndRef = useRef<HTMLDivElement>(null);

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

  const pillClass = connState === "connected"
    ? "lg-pill lg-pill--connected"
    : connState === "connecting"
    ? "lg-pill lg-pill--connecting"
    : "lg-pill lg-pill--disconnected";
  const pillLabel = connState === "connected" ? "Connected" : connState === "connecting" ? "Connecting…" : "Offline";

  return (
    <div style={{ height: "100vh", background: "var(--bg)", display: "flex", overflow: "hidden" }}>
      {/* Sidebar */}
      {showSidebar && <AppSidebar />}

      {/* Main */}
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
        {/* Topbar */}
        <div style={{
          height: 52, flexShrink: 0, display: "flex", alignItems: "center",
          justifyContent: "space-between", padding: "0 16px",
          borderBottom: "1px solid var(--line-700)",
          background: "var(--glass-opaque)",
        }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <button className="lg-iconbtn lg-iconbtn--md" onClick={() => setShowSidebar(s => !s)}>
              <PanelLeft size={18} />
            </button>
            <span style={{
              fontFamily: "var(--font-display)", fontWeight: 600, fontSize: 16,
              background: "var(--grad-brand-h)", WebkitBackgroundClip: "text",
              backgroundClip: "text", WebkitTextFillColor: "transparent",
            }}>Chat</span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            {isLoading && (
              <button
                onClick={() => setIsPaused(!isPaused)}
                className="lg-btn lg-btn--ghost lg-btn--sm"
              >
                {isPaused ? "Resume" : "Pause"}
              </button>
            )}
            <div className={pillClass}>
              <span className="lg-pill__dot" />
              {pillLabel}
            </div>
          </div>
        </div>

        {/* Connection warning */}
        {!isConnected && (
          <div style={{ padding: "8px 16px", background: "var(--red-dim)", borderBottom: "1px solid rgba(239,68,68,.3)" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, maxWidth: 720, margin: "0 auto" }}>
              <span style={{ color: "var(--red-500)", fontWeight: 600 }}>!</span>
              <span style={{ fontSize: 13, color: "var(--text-body)" }}>
                {connState === "connecting" ? "Connecting to agent on :9090…" : "Agent disconnected. Start lagado-agent to enable chat."}
              </span>
              <button onClick={() => navigate("/server")} style={{ marginLeft: "auto", fontSize: 12, color: "var(--red-500)", background: "none", border: "none", cursor: "pointer", textDecoration: "underline" }}>
                Configure
              </button>
            </div>
          </div>
        )}

        {/* Messages */}
        <div style={{ flex: 1, overflowY: "auto", padding: 24 }}>
          <div style={{ maxWidth: 720, margin: "0 auto", display: "flex", flexDirection: "column", gap: 20 }}>
            {messages.map(msg => (
              <div key={msg.id}>
                {msg.role === "assistant" ? (
                  <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
                    <img src="/lagado-mark.png" width={28} height={28} alt="Lagado" style={{ flexShrink: 0, filter: "drop-shadow(0 0 6px rgba(139,92,246,.35))" }} />
                    {msg.content.startsWith("$ ") ? (
                      /* Command activity — render as a terminal block, distinct from chat */
                      <pre style={{ flex: 1, margin: 0, padding: "8px 12px", background: "var(--surface)", border: "1px solid var(--line-700)", borderRadius: 8, fontFamily: "var(--font-mono)", fontSize: 12.5, lineHeight: 1.5, color: "var(--text-dim)", whiteSpace: "pre-wrap", overflowX: "auto" }}>
                        {msg.content}
                      </pre>
                    ) : (
                      <div className="lg-bubble lg-bubble--agent" style={{ paddingLeft: 20 }}>
                        <p style={{ whiteSpace: "pre-wrap" }}>{msg.content}</p>
                      </div>
                    )}
                  </div>
                ) : (
                  <div style={{ display: "flex", justifyContent: "flex-end" }}>
                    <div className="lg-bubble lg-bubble--user">
                      <p style={{ whiteSpace: "pre-wrap" }}>{msg.content}</p>
                    </div>
                  </div>
                )}
              </div>
            ))}

            {isLoading && (
              <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                <img src="/lagado-mark.png" width={28} height={28} alt="Lagado" style={{ flexShrink: 0, filter: "drop-shadow(0 0 6px rgba(139,92,246,.35))" }} />
                <div className="lg-bubble lg-bubble--agent" style={{ paddingLeft: 20, display: "flex", alignItems: "center", gap: 11 }}>
                  <HyperLoader size={28} />
                  <span style={{ fontSize: 13, color: "var(--text-dim)", fontFamily: "var(--font-mono)" }}>thinking…</span>
                </div>
              </div>
            )}

            {pendingPermission && (
              <PermissionCard
                req={pendingPermission}
                onApprove={() => approve(pendingPermission.id)}
                onDeny={() => deny(pendingPermission.id)}
                onSwitch={surface => navigate(surfaceRoutes[surface] ?? "/chat")}
              />
            )}
            <div ref={messagesEndRef} />
          </div>
        </div>

        {/* Composer */}
        <div style={{ flexShrink: 0, borderTop: "1px solid var(--line-700)", background: "var(--surface)", padding: 16 }}>
          <div style={{ maxWidth: 720, margin: "0 auto" }}>
            <div className="lg-glasspanel" style={{ padding: 8, display: "flex", alignItems: "flex-end", gap: 6 }}>
              <button className="lg-iconbtn lg-iconbtn--md" aria-label="Attach">
                <Paperclip size={18} />
              </button>
              <textarea
                value={input}
                onChange={e => setInput(e.target.value)}
                onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
                placeholder="Message Lagado…  (stays on this machine)"
                rows={1}
                style={{
                  flex: 1, background: "transparent", border: "none", outline: "none",
                  resize: "none", color: "var(--text-body)", fontFamily: "var(--font-body)",
                  fontSize: 14, padding: "9px 4px", lineHeight: 1.4, maxHeight: 120,
                }}
              />
              <button className="lg-iconbtn lg-iconbtn--md" aria-label="Voice">
                <Mic size={18} />
              </button>
              {isLoading ? (
                /* While the agent runs, the send control becomes a STOP (abort) — the user can always
                   halt a running agent (the §4 send→stop morph). */
                <button
                  onClick={stop}
                  className="lg-iconbtn lg-iconbtn--md"
                  aria-label="Stop"
                  style={{ background: "var(--surface-raised)", color: "var(--text-body)", borderRadius: 12 }}
                >
                  <Square size={15} fill="currentColor" />
                </button>
              ) : (
                <button
                  onClick={handleSend}
                  disabled={!input.trim()}
                  className="lg-iconbtn lg-iconbtn--md"
                  aria-label="Send"
                  style={{
                    background: input.trim() ? "var(--grad-brand-h)" : "var(--surface-raised)",
                    color: input.trim() ? "#fff" : "var(--text-dim)",
                    borderRadius: 12,
                  }}
                >
                  <ArrowUp size={18} />
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
