import { useEffect } from "react";
import { wsClient, type WebEvent } from "@/lib/websocket";
import { showMailNotification } from "@/lib/browser-notifications";

export function useNotificationOpenNavigation() {
  useEffect(() => {
    const handler = (event: WebEvent) => {
      const payload = event.payload as { message_id?: string; subject?: string; from?: string } | undefined;
      if (payload) void showMailNotification(event.type, payload).catch(() => {});
    };
    wsClient.on("mail:new", handler);
    wsClient.on("mail:unsnoozed", handler);
    return () => { wsClient.off("mail:new", handler); wsClient.off("mail:unsnoozed", handler); };
  }, []);
}
