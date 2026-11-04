import { useState } from "react";
import { useTranslation } from "react-i18next";
import { setRealtimePreference, showTestNotification } from "@/lib/api";
import { reloadServerPreference } from "@/lib/server-preferences";
import { requestBrowserNotifications } from "@/lib/browser-notifications";
import { useToastStore } from "@/stores/toast.store";
import { useUIStore, type RealtimePreference } from "@/stores/ui.store";

const MODES: RealtimePreference[] = ["realtime", "balanced", "battery", "manual"];

export default function GeneralTab() {
  const { t } = useTranslation();
  const addToast = useToastStore(s => s.addToast);
  const mode = useUIStore(s => s.realtimeMode);
  const ready = useUIStore(s => s.serverPreferencesReady);
  const notifications = useUIStore(s => s.notificationsEnabled);
  const showUnread = useUIStore(s => s.showFolderUnreadCount);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const report = (error: unknown) => addToast({ type: "error", message: error instanceof Error ? error.message : String(error) });

  async function updateMode(next: RealtimePreference) {
    setSaving(true);
    try {
      await setRealtimePreference(next);
      await reloadServerPreference();
    } catch (error) { report(error); } finally { setSaving(false); }
  }
  async function reloadPreference() {
    try {
      await reloadServerPreference();
    } catch (error) { report(error); }
  }
  async function toggleNotifications() {
    try {
      if (!notifications) await requestBrowserNotifications();
      useUIStore.getState().setNotificationsEnabled(!notifications);
    } catch (error) { report(error); }
  }
  async function testNotification() {
    setTesting(true);
    try { await showTestNotification(); } catch (error) { report(error); } finally { setTesting(false); }
  }

  return <div style={{ fontSize: 13, color: "var(--color-text-primary)" }}>
    <h3>{t("settings.realtimeMode", "Realtime Mode")}</h3>
    <p style={{ color: "var(--color-text-secondary)", lineHeight: 1.6 }}>{t("web.serverSync", "This setting controls the server, even when all browser tabs are closed. Manual mode only syncs when you request it.")}</p>
    {!ready && <button type="button" onClick={reloadPreference}>{t("web.reloadServerSettings", "Load server settings")}</button>}
    <div role="group" aria-label={t("settings.realtimeMode", "Realtime Mode")} style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 8 }}>
      {MODES.map(option => <button key={option} type="button" disabled={!ready || saving} aria-pressed={mode === option} onClick={() => updateMode(option)} style={{
        padding: 14, borderRadius: 6, textAlign: "left", color: "var(--color-text-primary)",
        border: mode === option ? "2px solid var(--color-accent)" : "1px solid var(--color-border)",
        background: mode === option ? "var(--color-bg-hover)" : "transparent", cursor: ready && !saving ? "pointer" : "not-allowed",
      }}>{t(`web.syncMode.${option}`)}</button>)}
    </div>
    <h3 style={{ marginTop: 28 }}>{t("settings.notifications")}</h3>
    <p style={{ color: "var(--color-text-secondary)", lineHeight: 1.6 }}>{t("web.notificationScope", "Notifications require browser permission and an open Pebble tab. Server sync, rules, and snooze reminders continue without a browser, but closed-browser push delivery is not supported.")}</p>
    <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
      <input type="checkbox" checked={notifications} onChange={toggleNotifications} />
      {t("settings.enableNotifications")}
    </label>
    <button type="button" onClick={testNotification} disabled={!notifications || testing} style={{ marginTop: 12 }}>
      {testing ? t("settings.testNotificationSending", "Sending test notification...") : t("settings.testNotification", "Send test notification")}
    </button>
    <h3 style={{ marginTop: 28 }}>{t("settings.folderCounts", "Folder Counts")}</h3>
    <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
      <input type="checkbox" checked={showUnread} onChange={() => useUIStore.getState().setShowFolderUnreadCount(!showUnread)} />
      {t("settings.showUnreadCount", "Show unread count badges in sidebar")}
    </label>
  </div>;
}
