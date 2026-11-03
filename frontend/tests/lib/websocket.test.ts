import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

class Socket {
  static instances: Socket[] = [];
  onopen?: () => void;
  onmessage?: (event: { data: string }) => void;
  onclose?: (event: { code: number }) => void;
  onerror?: () => void;
  send = vi.fn();
  close = vi.fn(() => this.onclose?.({ code: 1000 }));
  constructor(public url: string) { Socket.instances.push(this); }
  message(value: unknown) { this.onmessage?.({ data: JSON.stringify(value) }); }
}

describe("authenticated WebSocket lifecycle", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", Socket);
    Socket.instances = [];
    localStorage.setItem("pebble_token", "test-session-token");
  });
  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); localStorage.clear(); });

  it("authenticates with the first message, never the URL, and forwards payloads after auth", async () => {
    const { wsClient } = await import("@/lib/websocket");
    const event = vi.fn();
    wsClient.on("mail:new", event);
    wsClient.connect();
    wsClient.connect();
    expect(Socket.instances).toHaveLength(1);
    const socket = Socket.instances[0];
    expect(socket.url).not.toContain("test-session-token");
    socket.onopen?.();
    expect(socket.send).toHaveBeenCalledWith("test-session-token");
    socket.message({ type: "mail:new", payload: { message_id: "blocked" } });
    expect(event).not.toHaveBeenCalled();
    socket.message({ type: "authenticated" });
    socket.message({ type: "mail:new", payload: { message_id: "safe" } });
    expect(event).toHaveBeenCalledWith({ type: "mail:new", payload: { message_id: "safe" } });
    wsClient.disconnect();
    vi.advanceTimersByTime(60000);
    expect(Socket.instances).toHaveLength(1);
  });

  it("retries network failures but never an explicit authentication rejection", async () => {
    const { wsClient } = await import("@/lib/websocket");
    wsClient.connect();
    Socket.instances[0].onclose?.({ code: 1006 });
    vi.advanceTimersByTime(1000);
    expect(Socket.instances).toHaveLength(2);
    Socket.instances[1].message({ type: "error" });
    vi.advanceTimersByTime(60000);
    expect(Socket.instances).toHaveLength(2);
    wsClient.disconnect();
  });
});
