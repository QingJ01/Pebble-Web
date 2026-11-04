import { invoke } from "@/lib/web-commands";

const LEGACY_STORAGE_KEY = "pebble-signatures";

function readLegacySignatures(): Record<string, string> | null {
  try {
    const raw = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    if (!Object.values(parsed).every((value) => typeof value === "string")) return null;
    return parsed as Record<string, string>;
  } catch {
    return null;
  }
}

function clearLegacySignature(accountId: string) {
  try {
    const signatures = readLegacySignatures();
    if (!signatures || !Object.prototype.hasOwnProperty.call(signatures, accountId)) return;
    delete signatures[accountId];
    if (Object.keys(signatures).length === 0) {
      localStorage.removeItem(LEGACY_STORAGE_KEY);
    } else {
      localStorage.setItem(LEGACY_STORAGE_KEY, JSON.stringify(signatures));
    }
  } catch { /* ignored */ }
}

export async function getSignature(accountId: string): Promise<string> {
  const legacySignatures = readLegacySignatures();
  const hasLegacySignature = Boolean(
    legacySignatures && Object.prototype.hasOwnProperty.call(legacySignatures, accountId),
  );
  const signature = await invoke<string>("get_email_signature", { accountId });

  if (signature || !hasLegacySignature || !legacySignatures) {
    if (signature) clearLegacySignature(accountId);
    return signature;
  }

  const legacySignature = legacySignatures[accountId];
  try {
    const migratedSignature = await invoke<string>("migrate_email_signature_if_absent", {
      accountId,
      signature: legacySignature,
    });
    clearLegacySignature(accountId);
    return migratedSignature;
  } catch (error) {
    console.warn("Failed to migrate legacy email signature:", error);
  }
  return legacySignature;
}

export async function setSignature(accountId: string, signature: string): Promise<void> {
  await invoke<void>("set_email_signature", { accountId, signature });
  clearLegacySignature(accountId);
}
