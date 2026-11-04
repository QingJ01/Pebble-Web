import { wsClient, type WebEvent } from "./websocket";

export async function listen<T>(name: string, callback: (event: { payload: T }) => void): Promise<() => void> {
  const handler = (event: WebEvent) => callback({ payload: (event.payload ?? event) as T });
  wsClient.on(name, handler);
  return () => wsClient.off(name, handler);
}
