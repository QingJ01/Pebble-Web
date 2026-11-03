import { useEffect } from "react";
import { invoke } from "@/lib/web-commands";
import { listen } from "@/lib/web-events";
import { wsClient } from "@/lib/websocket";
import { useUIStore, realtimePreferenceToPollInterval, type RealtimePreference } from "@/stores/ui.store";

export function useRealtimePreferenceSync() {
  useEffect(() => {
    let active = true;
    let revision = 0;
    const apply = (mode: RealtimePreference) => {
      if (!active || !["realtime", "balanced", "battery", "manual"].includes(mode)) return;
      useUIStore.setState({ realtimeMode: mode, pollInterval: realtimePreferenceToPollInterval(mode), serverPreferencesReady: true });
    };
    const unlisten = listen<{ mode: RealtimePreference }>("app:realtime-preference", ({ payload }) => {
      revision += 1;
      apply(payload.mode);
    });
    const reload = () => {
      const requestedRevision = ++revision;
      void invoke<RealtimePreference>("get_realtime_preference").then(mode => {
        if (revision === requestedRevision) apply(mode);
      }).catch(() => {
        if (active && revision === requestedRevision) useUIStore.setState({ serverPreferencesReady: false });
      });
    };
    reload();
    wsClient.on("connection:ready", reload);
    return () => { active = false; wsClient.off("connection:ready", reload); void unlisten.then(fn => fn()); };
  }, []);
}
