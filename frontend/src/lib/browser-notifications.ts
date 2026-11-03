import i18n from "./i18n";
import { isComposeDirty } from "@/stores/compose.store";
import { useConfirmStore } from "@/stores/confirm.store";
import { useUIStore } from "@/stores/ui.store";

export async function requestBrowserNotifications(): Promise<void> {
  if (!("Notification" in window) || !window.isSecureContext) {
    throw new Error(i18n.t("web.notificationsUnavailable", "Notifications require a supported browser and HTTPS (or localhost)."));
  }
  const permission = Notification.permission === "default" ? await Notification.requestPermission() : Notification.permission;
  if (permission !== "granted") throw new Error(i18n.t("web.notificationsDenied", "Allow notifications in this site's browser permissions."));
}

export async function showBrowserTestNotification(): Promise<void> {
  await requestBrowserNotifications();
  const notification = new Notification("Pebble", { body: i18n.t("web.notificationTest", "Browser notifications are working."), tag: "pebble-test" });
  setTimeout(() => notification.close(), 10000);
}

interface MailNotification { message_id?: string; subject?: string; from?: string }

export async function showMailNotification(kind: string, payload: MailNotification): Promise<void> {
  if (!("Notification" in window) || Notification.permission !== "granted" || !useUIStore.getState().notificationsEnabled || !payload.message_id) return;
  const id = `${kind}:${payload.message_id}`;
  const show = () => {
    const key = "pebble-notification-receipts";
    const now = Date.now();
    let receipts: Record<string, number> = {};
    try {
      const parsed: unknown = JSON.parse(localStorage.getItem(key) ?? "{}");
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        receipts = Object.fromEntries(Object.entries(parsed).filter(([, time]) => typeof time === "number" && time > now - 60000));
      }
    } catch { /* Notification tags still deduplicate when storage is unavailable. */ }
    if (receipts[id]) return;
    receipts[id] = now;
    try { localStorage.setItem(key, JSON.stringify(Object.fromEntries(Object.entries(receipts).slice(-128)))); } catch { /* Optional browser storage. */ }
    const notice = new Notification(kind === "mail:unsnoozed" ? i18n.t("web.snoozeReminder", "Snoozed mail is ready") : "Pebble", {
      body: payload.subject || i18n.t("web.newMail", "You have new mail."), tag: `pebble:${id}`,
    });
    notice.onclick = async () => {
      window.focus();
      if (isComposeDirty() && !(await useConfirmStore.getState().confirm({
        title: i18n.t("compose.discardDraft", "Discard draft"),
        message: i18n.t("compose.discardDraftConfirm", "You have an unsaved draft. Discard and leave?"),
        destructive: true,
      }))) return;
      useUIStore.getState().openMessageInInbox(payload.message_id!);
      notice.close();
    };
  };
  if (navigator.locks) await navigator.locks.request("pebble-notifications", show);
  else show();
}
