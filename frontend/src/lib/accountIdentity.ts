import type { Account } from "./ipc-types";

export function accountLabel(account: Account): string {
  return account.account_label?.trim() || account.email;
}

export function accountOptionLabel(account: Account): string {
  const label = account.account_label?.trim();
  return label ? `${label} · ${account.email}` : account.email;
}

export function senderIdentityLabel(account: Account): string {
  const name = (account.provider === "outlook" ? account.provider_display_name : account.display_name)?.trim();
  return name ? `${name} <${account.email}>` : account.email;
}
