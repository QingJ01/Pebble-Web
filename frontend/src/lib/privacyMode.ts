import type { PrivacyMode } from "@/lib/api";
import { profileLocalStorage } from "@/lib/profileStorage";

export const PRIVACY_MODE_KEY = "pebble-privacy-mode";
export const DEFAULT_STORED_PRIVACY_MODE = "relaxed";

export type StoredPrivacyMode = "strict" | "relaxed" | "off";

export function isStoredPrivacyMode(value: string | null): value is StoredPrivacyMode {
  return value === "strict" || value === "relaxed" || value === "off";
}

export function readStoredPrivacyMode(
  storage: Pick<Storage, "getItem"> = profileLocalStorage,
): StoredPrivacyMode {
  const saved = storage.getItem(PRIVACY_MODE_KEY);
  return isStoredPrivacyMode(saved) ? saved : DEFAULT_STORED_PRIVACY_MODE;
}

export function privacyModeToApi(mode: StoredPrivacyMode): PrivacyMode {
  switch (mode) {
    case "strict":
      return "Strict";
    case "off":
      return "Off";
    case "relaxed":
      return "LoadOnce";
  }
}

export function defaultPrivacyMode(): PrivacyMode {
  return privacyModeToApi(readStoredPrivacyMode());
}
