import { useRef, useState, useEffect, useCallback } from 'react';

// ── Types ─────────────────────────────────────────────────────────────────────

export type ConnState   = 'connecting' | 'connected' | 'disconnected';
export type AgentCmd    = 'pause' | 'resume' | 'stop';

export interface PermissionRequest {
  action: string;
  tool:   string;
}

export interface AgentSocket {
  connState:    ConnState;
  sendGoal:     (goal: string) => void;
  sendCommand:  (cmd: AgentCmd) => void;
  sendRaw:      (msg: string) => void;
  disconnect:   () => void;
}

export interface AgentSocketOptions {
  /** Called when the agent sends a permission:<json> message. */
  onPermissionRequest?: (req: PermissionRequest) => void;
  /** Called for every non-permission message (action log, status, etc.). */
  onMessage?: (raw: string) => void;
}

// ── Constants ─────────────────────────────────────────────────────────────────

const WS_URL      = 'ws://127.0.0.1:9090';
const MAX_RETRIES = 5;
const RETRY_DELAY = 2000;

// ── Hook ──────────────────────────────────────────────────────────────────────

export function useAgentSocket(options: AgentSocketOptions = {}): AgentSocket {
  // Keep options in a ref so callbacks never cause reconnects
  const optsRef = useRef(options);
  optsRef.current = options;

  const [connState, setConnState] = useState<ConnState>('disconnected');

  const wsRef       = useRef<WebSocket | null>(null);
  const queueRef    = useRef<string[]>([]);
  const retriesRef  = useRef(0);
  const retryTimer  = useRef<ReturnType<typeof setTimeout> | null>(null);
  const intentional = useRef(false);

  // ── Core connect ─────────────────────────────────────────────────────────

  const connect = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.onopen    = null;
      wsRef.current.onmessage = null;
      wsRef.current.onerror   = null;
      wsRef.current.onclose   = null;
      wsRef.current.close();
      wsRef.current = null;
    }

    intentional.current = false;
    setConnState('connecting');

    let ws: WebSocket;
    try {
      ws = new WebSocket(WS_URL);
    } catch {
      setConnState('disconnected');
      return;
    }

    ws.onopen = () => {
      retriesRef.current = 0;
      setConnState('connected');
      const q = queueRef.current.splice(0);
      for (const msg of q) {
        try { ws.send(msg); } catch { /* ignore */ }
      }
    };

    ws.onclose = () => {
      wsRef.current = null;
      if (intentional.current) { setConnState('disconnected'); return; }
      if (retriesRef.current < MAX_RETRIES) {
        retriesRef.current += 1;
        setConnState('connecting');
        retryTimer.current = setTimeout(connect, RETRY_DELAY);
      } else {
        setConnState('disconnected');
      }
    };

    ws.onerror = () => { /* onclose fires right after */ };

    ws.onmessage = (ev) => {
      const raw = String(ev.data);
      console.log('[laputa-agent]', raw);

      // ── Permission gate intercept ───────────────────────────────────────
      if (raw.startsWith('permission:')) {
        const jsonPart = raw.slice('permission:'.length).trim();
        try {
          const req = JSON.parse(jsonPart) as PermissionRequest;
          optsRef.current.onPermissionRequest?.(req);
        } catch {
          // Malformed permission message — surface as plain message
          optsRef.current.onMessage?.(raw);
        }
        return;
      }

      // ── Action log / status messages ────────────────────────────────────
      optsRef.current.onMessage?.(raw);
    };

    wsRef.current = ws;
  }, []);

  // ── Mount / unmount ───────────────────────────────────────────────────────

  useEffect(() => {
    connect();
    return () => {
      intentional.current = true;
      if (retryTimer.current) clearTimeout(retryTimer.current);
      wsRef.current?.close();
    };
  }, [connect]);

  // ── Public API ────────────────────────────────────────────────────────────

  const send = useCallback((msg: string) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(msg);
    } else {
      queueRef.current.push(msg);
      if (connState === 'disconnected') {
        retriesRef.current = 0;
        connect();
      }
    }
  }, [connState, connect]);

  const sendGoal    = useCallback((goal: string)   => send(`goal:${goal}`), [send]);
  const sendCommand = useCallback((cmd: AgentCmd)   => send(cmd),           [send]);
  const sendRaw     = useCallback((msg: string)     => send(msg),           [send]);

  const disconnect  = useCallback(() => {
    intentional.current = true;
    if (retryTimer.current) clearTimeout(retryTimer.current);
    wsRef.current?.close();
    wsRef.current = null;
    queueRef.current = [];
    setConnState('disconnected');
  }, []);

  return { connState, sendGoal, sendCommand, sendRaw, disconnect };
}
