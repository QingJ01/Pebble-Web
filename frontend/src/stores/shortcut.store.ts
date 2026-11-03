import { create } from "zustand";
import { profileLocalStorage } from "@/lib/profileStorage";

export interface ShortcutBinding {
  actionId: string;
  keys: string;
}

interface ShortcutState {
  bindings: Record<string, string>;
  recording: string | null;
  updateShortcut: (actionId: string, keys: string) => void;
  resetToDefaults: () => void;
  startRecording: (actionId: string) => void;
  stopRecording: () => void;
  detectConflict: (keys: string, excludeAction?: string) => string | null;
}

const DEFAULT_BINDINGS: Record<string, string> = {
  "command-palette": "Ctrl+K",
  "close-modal": "Escape",
  "next-message": "J",
  "prev-message": "K",
  "open-message": "Enter",
  "toggle-star": "S",
  "archive-message": "E",
  "compose-new": "C",
  "reply": "R",
  "reply-all": "A",
  "forward": "F",
  "focus-search": "/",
  "toggle-view-inbox": "Ctrl+Shift+I",
  "toggle-view-kanban": "Ctrl+Shift+K",
  "open-search": "Ctrl+Shift+F",
  "open-cloud-settings": "Ctrl+Shift+B",
  "toggle-notifications": "Ctrl+Shift+N",
  "translate-selection": "Shift+Alt+T",
  "toggle-bilingual": "Shift+Alt+B",
};

const STORAGE_KEY = "pebble-shortcuts";

function loadBindings(): Record<string, string> {
  try {
    const saved = profileLocalStorage.getItem(STORAGE_KEY);
    if (saved) return { ...DEFAULT_BINDINGS, ...JSON.parse(saved) };
  } catch {
    // ignore parse errors
  }
  return { ...DEFAULT_BINDINGS };
}

export const useShortcutStore = create<ShortcutState>((set, get) => ({
  bindings: loadBindings(),
  recording: null,
  updateShortcut: (actionId, keys) => {
    const newBindings = { ...get().bindings, [actionId]: keys };
    profileLocalStorage.setItem(STORAGE_KEY, JSON.stringify(newBindings));
    set({ bindings: newBindings, recording: null });
  },
  resetToDefaults: () => {
    profileLocalStorage.removeItem(STORAGE_KEY);
    set({ bindings: { ...DEFAULT_BINDINGS } });
  },
  startRecording: (actionId) => set({ recording: actionId }),
  stopRecording: () => set({ recording: null }),
  detectConflict: (keys, excludeAction) => {
    const { bindings } = get();
    for (const [action, bound] of Object.entries(bindings)) {
      if (action !== excludeAction && bound.toLowerCase() === keys.toLowerCase()) {
        return action;
      }
    }
    return null;
  },
}));

export { DEFAULT_BINDINGS };
