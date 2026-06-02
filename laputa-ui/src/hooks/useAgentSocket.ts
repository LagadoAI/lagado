import { useRef, useState, useEffect, useCallback } from 'react';

// ── Types ─────────────────────────────────────────────────────────────────────

export type ConnState   = 'connecting' | 'connected' | 'disconnected';
export type AgentCmd    = 'pause' | 'resume' | 'stop';

export interface Envelope {
  v:       number;
  kind:    string;
  payload: any;
}

export interface PermissionRequest {
  id:             string;
  type:           'tap' | 'typed';
  tool:           string;
  action:         string;
  reason:         string;
  origin_surface: string;
  origin_agent:   string;
}

export interface AgentSocket {
  connState:     ConnState;
  sendGoal:      (goal: string) => void;
  sendCommand:   (cmd: AgentCmd) => void;
  sendApproval:  (id: string, approved: boolean) => void;
  sendRaw:       (msg: string) => void;
  disconnect:    () => void;
}

export interface AgentSocketOptions {
  onPermissionRequest?: (req: PermissionRequest) => void;
  onMessage?:           (text: string) => void;
  onStatus?:            (s: { state: string; detail: string }) => void;
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

      let env: Envelope;
      try {
        env = JSON.parse(raw) as Envelope;
      } catch {
        console.warn('[laputa-agent] non-JSON message ignored:', raw);
        return;
      }

      if (env.v !== 1) {
        console.warn('[laputa-agent] unknown envelope version:', env.v);
        return;
      }

      switch (env.kind) {
        case 'permission':
          optsRef.current.onPermissionRequest?.(env.payload as PermissionRequest);
          break;
        case 'action_log':
          optsRef.current.onMessage?.(env.payload.text ?? '');
          break;
        case 'status':
          optsRef.current.onStatus?.({
            state:  env.payload.state  ?? '',
            detail: env.payload.detail ?? '',
          });
          break;
        default:
          console.warn('[laputa-agent] unknown envelope kind:', env.kind);
      }
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

  const sendGoal = useCallback(
    (goal: string) => send(JSON.stringify({ v: 1, kind: 'goal', payload: { text: goal } })),
    [send],
  );

  const sendCommand = useCallback(
    (cmd: AgentCmd) => send(JSON.stringify({ v: 1, kind: 'command', payload: { cmd } })),
    [send],
  );

  const sendApproval = useCallback(
    (id: string, approved: boolean) =>
      send(JSON.stringify({ v: 1, kind: 'approval', payload: { id, approved } })),
    [send],
  );

  const sendRaw = useCallback((msg: string) => send(msg), [send]);

  const disconnect = useCallback(() => {
    intentional.current = true;
    if (retryTimer.current) clearTimeout(retryTimer.current);
    wsRef.current?.close();
    wsRef.current = null;
    queueRef.current = [];
    setConnState('disconnected');
  }, []);

  return { connState, sendGoal, sendCommand, sendApproval, sendRaw, disconnect };
}
