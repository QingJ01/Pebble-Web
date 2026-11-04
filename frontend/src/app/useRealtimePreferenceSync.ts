import { useEffect } from "react";
import { listen } from "@/lib/web-events";
import { wsClient } from "@/lib/websocket";
import { applyServerPreference, invalidatePreferenceRequests, reloadServerPreference } from "@/lib/server-preferences";
import type { RealtimePreference } from "@/stores/ui.store";

export function useRealtimePreferenceSync() {
  useEffect(() => {
    let active = true;
    const unlisten = listen<{ mode: RealtimePreference }>("app:realtime-preference", ({ payload }) => {
      if (active) applyServerPreference(payload.mode);
    });
    const reload = () => {
      void reloadServerPreference().catch(() => {});
    };
    reload();
    wsClient.on("connection:ready", reload);
    return () => { active = false; invalidatePreferenceRequests(); wsClient.off("connection:ready", reload); void unlisten.then(fn => fn()); };
  }, []);
}
