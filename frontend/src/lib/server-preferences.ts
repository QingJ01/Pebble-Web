import { invoke } from "./web-commands";
import { useUIStore, realtimePreferenceToPollInterval, type RealtimePreference } from "@/stores/ui.store";

let revision = 0;

export function invalidatePreferenceRequests() {
  revision += 1;
}

export function applyServerPreference(mode: RealtimePreference) {
  if (!["realtime", "balanced", "battery", "manual"].includes(mode)) {
    throw new Error("Invalid server preference.");
  }
  invalidatePreferenceRequests();
  useUIStore.setState({ realtimeMode: mode, pollInterval: realtimePreferenceToPollInterval(mode), serverPreferencesReady: true });
}

export async function reloadServerPreference(): Promise<void> {
  const requestedRevision = ++revision;
  try {
    const mode = await invoke<RealtimePreference>("get_realtime_preference");
    if (requestedRevision === revision) applyServerPreference(mode);
  } catch (error) {
    if (requestedRevision === revision) useUIStore.setState({ serverPreferencesReady: false });
    throw error;
  }
}
