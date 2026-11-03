import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { wsClient, type WebEvent } from "@/lib/websocket";

export function useRealtimeSyncTriggers() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const handler = (event: WebEvent) => {
      if (["connection:ready", "sync_complete", "new_mail", "mail:folder-changed"].includes(event.type)) {
        for (const key of ["accounts", "messages", "folders", "threads", "folder-unread-counts", "snoozed", "kanban", "pending-mail-ops"]) {
          void queryClient.invalidateQueries({ queryKey: [key] });
        }
      }
    };
    wsClient.on("*", handler);
    wsClient.connect();
    return () => { wsClient.off("*", handler); wsClient.disconnect(); };
  }, [queryClient]);
}
