export interface WebEvent { type: string; payload?: unknown; [key: string]: unknown }
type EventHandler = (data: WebEvent) => void;

class WebSocketClient {
  private ws: WebSocket | null = null;
  private handlers = new Map<string, Set<EventHandler>>();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private active = false;
  private attempts = 0;

  connect() {
    this.active = true;
    if (this.ws || this.reconnectTimer) return;
    const token = localStorage.getItem("pebble_token");
    if (!token) return;
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(`${protocol}//${location.host}/api/v1/ws`);
    this.ws = socket;
    let authenticated = false;
    let rejected = false;
    socket.onopen = () => socket.send(token);
    socket.onmessage = (event) => {
      if (this.ws !== socket) return;
      let message: WebEvent;
      try { message = JSON.parse(event.data); } catch { return; }
      if (!authenticated) {
        if (message.type === "authenticated") {
          authenticated = true;
          this.attempts = 0;
          this.dispatch({ type: "connection:ready" });
        } else if (message.type === "error") {
          rejected = true;
          socket.close();
        }
        return;
      }
      this.dispatch(message);
    };
    socket.onclose = (event) => {
      if (this.ws !== socket) return;
      this.ws = null;
      if (!this.active || rejected || event.code === 1008 || !localStorage.getItem("pebble_token")) return;
      this.reconnectTimer = setTimeout(() => {
        this.reconnectTimer = null;
        this.connect();
      }, Math.min(30000, 1000 * 2 ** this.attempts++));
    };
    socket.onerror = () => socket.close();
  }

  private dispatch(event: WebEvent) {
    for (const handler of [...(this.handlers.get(event.type) ?? []), ...(this.handlers.get("*") ?? [])]) {
      try { handler(event); } catch (error) { console.warn("Realtime event handler failed", error); }
    }
  }

  disconnect() {
    this.active = false;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
    const socket = this.ws;
    this.ws = null;
    socket?.close();
    this.attempts = 0;
  }

  on(event: string, handler: EventHandler) {
    const handlers = this.handlers.get(event) ?? new Set<EventHandler>();
    handlers.add(handler);
    this.handlers.set(event, handlers);
  }

  off(event: string, handler: EventHandler) {
    this.handlers.get(event)?.delete(handler);
  }
}

export const wsClient = new WebSocketClient();
