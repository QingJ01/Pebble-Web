import { profileLocalStorage } from "@/lib/profileStorage";

export const START_HIDDEN_TO_TRAY_KEY = "pebble-start-hidden-to-tray";

export function readStartHiddenToTrayPreference(storage: Pick<Storage, "getItem"> = profileLocalStorage): boolean {
  return storage.getItem(START_HIDDEN_TO_TRAY_KEY) === "true";
}

export function shouldShowMainWindowOnStartup(storage: Pick<Storage, "getItem"> = profileLocalStorage): boolean {
  return !readStartHiddenToTrayPreference(storage);
}
