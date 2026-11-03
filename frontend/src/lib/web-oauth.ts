import http from "@/api-client";
import type { Account } from "./ipc-types";

interface FlowStatus { status: "pending" | "completed" | "error"; account?: Account; error?: string }

export async function completeWebOAuth(provider: string, options: Record<string, unknown>): Promise<Account> {
  if (provider !== "gmail" && provider !== "outlook") throw new Error("Unsupported OAuth provider");
  const popup = window.open("about:blank", "_blank", "popup,width=600,height=750");
  if (!popup) throw new Error("Allow popups for this site to connect your account.");
  popup.opener = null;
  try {
    const { data: flow } = await http.post<{ authorization_url: string; flow_id: string }>(
      `/oauth/${provider}/start`, options, { timeout: 15000 },
    );
    const authorization = new URL(flow.authorization_url);
    const allowedHost = provider === "gmail" ? "accounts.google.com" : "login.microsoftonline.com";
    if (authorization.protocol !== "https:" || authorization.hostname !== allowedHost || authorization.username || authorization.password) {
      throw new Error("The server returned an invalid provider authorization URL.");
    }
    popup.location.replace(authorization.href);
    const expires = Date.now() + 10 * 60 * 1000;
    while (Date.now() < expires) {
      const { data } = await http.get<FlowStatus>(`/oauth/${provider}/status/${encodeURIComponent(flow.flow_id)}`, { timeout: 15000 });
      if (data.status === "completed" && data.account) return data.account;
      if (data.status === "error") throw new Error(data.error || "Account authorization failed.");
      if (popup.closed) throw new Error("Account authorization was cancelled.");
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
    throw new Error("Account authorization expired. Please try again.");
  } finally {
    popup.close();
  }
}
