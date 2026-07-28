/**
 * WebSocket client for the FRIDAY Mobile Protocol.
 *
 * Manages connection lifecycle, auth handshake, telemetry, events, and command
 * round-trips. Exposes reactive state via a simple callback-based store.
 */

import type { ServerMessage, ClientMessage, TelemetrySample, SystemHealth, ParsedEvent } from './protocol';
import { parseEvent } from './protocol';

export type ConnectionStatus = 'disconnected' | 'connecting' | 'authenticating' | 'connected' | 'reconnecting';

export interface ConnectionState {
  status: ConnectionStatus;
  sessionId: string | null;
  protocol: string;
  telemetry: TelemetrySample | null;
  health: SystemHealth | null;
  lastError: string | null;
}

export interface FridayEvent {
  id: string;
  topic: string;
  parsed: ParsedEvent;
  timestamp: number;
}

export type EventHandler = (event: FridayEvent) => void;
export type StateHandler = (state: ConnectionState) => void;
export type CommandResultHandler = (id: string, ok: boolean, result: unknown) => void;

let nextCmdId = 1;

export class FridayClient {
  private ws: WebSocket | null = null;
  private url: string;
  private token: string | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private maxReconnect = 10;
  private _state: ConnectionState = {
    status: 'disconnected',
    sessionId: null,
    protocol: '1.0',
    telemetry: null,
    health: null,
    lastError: null,
  };

  private onStateChange: StateHandler | null = null;
  private onEvent: EventHandler | null = null;
  private onCommandResult: CommandResultHandler | null = null;
  private eventListeners: Array<() => void> = [];

  constructor(url: string, token?: string) {
    this.url = url;
    this.token = token ?? null;
  }

  get state(): Readonly<ConnectionState> {
    return this._state;
  }

  connect() {
    if (this.ws) return;
    this.setState({ status: 'connecting', lastError: null });

    try {
      this.ws = new WebSocket(this.url);
    } catch {
      this.setState({ status: 'reconnecting', lastError: 'Failed to create WebSocket' });
      this.scheduleReconnect();
      return;
    }

    this.ws.onopen = () => {
      this.reconnectAttempts = 0;
      const hello: ClientMessage = {
        type: 'hello',
        client: {
          device: 'FRIDAY Desktop',
          model: navigator.userAgent,
          app_version: '0.1.0',
        },
        token: this.token,
        protocol: '1.0',
      };
      this.send(hello);
    };

    this.ws.onmessage = (msg) => {
      try {
        const data = JSON.parse(msg.data) as ServerMessage;
        this.handleMessage(data);
      } catch {
        // ignore malformed frames
      }
    };

    this.ws.onclose = () => {
      this.ws = null;
      if (this._state.status !== 'disconnected') {
        this.setState({ status: 'reconnecting' });
        this.scheduleReconnect();
      }
    };

    this.ws.onerror = () => {
      this.setState({ lastError: 'Connection error' });
    };
  }

  disconnect() {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.reconnectAttempts = this.maxReconnect; // prevent reconnect
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.setState({ status: 'disconnected' });
  }

  sendGoal(goal: string): string {
    const id = `cmd-${nextCmdId++}`;
    this.send({ type: 'command', id, method: 'planner.goal', params: { goal } });
    return id;
  }

  sendCommand(method: string, params: unknown = {}): string {
    const id = `cmd-${nextCmdId++}`;
    this.send({ type: 'command', id, method, params });
    return id;
  }

  onState(handler: StateHandler): () => void {
    this.onStateChange = handler;
    return () => { this.onStateChange = null; };
  }

  onEvents(handler: EventHandler): () => void {
    this.onEvent = handler;
    return () => { this.onEvent = null; };
  }

  onCommandResult(handler: CommandResultHandler): () => void {
    this.onCommandResult = handler;
    return () => { this.onCommandResult = null; };
  }

  private send(msg: ClientMessage) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  private handleMessage(msg: ServerMessage) {
    switch (msg.type) {
      case 'welcome':
        this._state.sessionId = msg.session_id;
        this._state.protocol = msg.protocol;
        if (msg.requires_auth) {
          this.setState({ status: 'authenticating' });
        } else {
          this.setState({ status: 'connected' });
        }
        break;

      case 'auth_complete':
        this.token = msg.token;
        this.setState({ status: 'connected' });
        break;

      case 'telemetry':
        this._state.telemetry = msg.sample;
        this.emitState();
        break;

      case 'health':
        this._state.health = msg.snapshot;
        this.emitState();
        break;

      case 'event': {
        const id = `evt-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
        const parsed = parseEvent(msg.topic, msg.payload);
        this.onEvent?.({ id, topic: msg.topic, parsed, timestamp: Date.now() });
        break;
      }

      case 'command_result':
        this.onCommandResult?.(msg.id, msg.ok, msg.result);
        break;

      case 'error':
        this.setState({ lastError: msg.message });
        break;
    }
  }

  private setState(partial: Partial<ConnectionState>) {
    Object.assign(this._state, partial);
    this.onStateChange?.({ ...this._state });
    this.emitState();
  }

  private emitState() {
    for (const fn of this.eventListeners) fn();
  }

  private scheduleReconnect() {
    if (this.reconnectAttempts >= this.maxReconnect) return;
    const delay = Math.min(1000 * 2 ** this.reconnectAttempts, 30000);
    this.reconnectAttempts++;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }
}
