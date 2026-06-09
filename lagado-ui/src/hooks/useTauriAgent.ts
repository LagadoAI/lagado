import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, useRef, useCallback, useState } from 'react';

// Keep the same ConnState union as useAgentSocket for full API compatibility
export type ConnState = 'connecting' | 'connected' | 'disconnected';
export type AgentCmd  = 'pause' | 'resume' | 'stop';

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
  connState:    ConnState;
  sendGoal:     (goal: string) => void;
  sendCommand:  (cmd: AgentCmd) => void;
  sendApproval: (id: string, approved: boolean) => void;
  sendRaw:      (msg: string) => void;
  disconnect:   () => void;
}

export interface AgentSocketOptions {
  onPermissionRequest?: (req: PermissionRequest) => void;
  onMessage?:           (text: string) => void;
  onStatus?:            (s: { state: string; detail: string }) => void;
}

export function useTauriAgent(options: AgentSocketOptions = {}): AgentSocket {
  // Tauri is always in-process — always connected
  const [connState] = useState<ConnState>('connected');
  const optsRef = useRef(options);
  optsRef.current = options;

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    Promise.all([
      listen<PermissionRequest>('permission', (event) => {
        optsRef.current.onPermissionRequest?.(event.payload);
      }),
      listen<{ text: string }>('action_log', (event) => {
        optsRef.current.onMessage?.(event.payload.text ?? '');
      }),
      listen<{ state: string; detail: string }>('status', (event) => {
        optsRef.current.onStatus?.({
          state:  event.payload.state  ?? '',
          detail: event.payload.detail ?? '',
        });
      }),
    ]).then((fns) => unlisteners.push(...fns));

    return () => { unlisteners.forEach((fn) => fn()); };
  }, []);

  const sendGoal = useCallback((goal: string) => {
    invoke('send_goal', { goal }).catch(console.error);
  }, []);

  const sendCommand = useCallback((cmd: AgentCmd) => {
    invoke('send_command', { cmd }).catch(console.error);
  }, []);

  const sendApproval = useCallback((id: string, approved: boolean) => {
    invoke('send_approval', { id, approved }).catch(console.error);
  }, []);

  // sendRaw and disconnect are no-ops in Tauri mode (in-process, always connected)
  const sendRaw   = useCallback((_msg: string) => {}, []);
  const disconnect = useCallback(() => {}, []);

  return { connState, sendGoal, sendCommand, sendApproval, sendRaw, disconnect };
}
