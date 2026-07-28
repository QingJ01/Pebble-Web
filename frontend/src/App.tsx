import { Component, type ErrorInfo, type ReactNode, useEffect, useState } from "react";
import i18next from "i18next";
import Layout from "./app/Layout";
import { logStartupTiming } from "@/lib/startupTiming";
import { isAuthenticated } from "./api-client";
import { LoginPage } from "./features/auth/LoginPage";
import { useUIStore } from "@/stores/ui.store";
import { useToastStore } from "@/stores/toast.store";
import { accountsQueryKey } from "@/hooks/queries";
import { queryClient } from "@/lib/query-client";

function consumeOAuthCallbackParams() {
  const params = new URLSearchParams(window.location.search);
  const oauth = params.get("oauth");
  if (!oauth) return;

  useUIStore.getState().setActiveView("settings");
  useUIStore.getState().setSettingsTab("accounts");

  if (oauth === "success") {
    const email = params.get("email");
    useToastStore.getState().addToast({
      message: email
        ? i18next.t("accountSetup.oauthSuccessWithEmail", "OAuth account added: {{email}}", { email })
        : i18next.t("accountSetup.oauthSuccess", "OAuth account added successfully"),
      type: "success",
    });
    void queryClient.invalidateQueries({ queryKey: accountsQueryKey });
  } else if (oauth === "error") {
    const message =
      params.get("message") ||
      i18next.t("accountSetup.oauthFailed", "OAuth sign-in failed");
    useToastStore.getState().addToast({
      message,
      type: "error",
    });
  }

  params.delete("oauth");
  params.delete("accountId");
  params.delete("email");
  params.delete("message");
  params.delete("tab");
  const next = params.toString();
  const cleanUrl = `${window.location.pathname}${next ? `?${next}` : ""}${window.location.hash}`;
  window.history.replaceState({}, "", cleanUrl);
}

class ErrorBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[ErrorBoundary]", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div style={{
          display: "flex", flexDirection: "column", alignItems: "center",
          justifyContent: "center", height: "100vh", gap: 16, padding: 24,
          fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
          color: "var(--color-text-primary, #333)",
          backgroundColor: "var(--color-bg, #fff)",
        }}>
          <h2 style={{ margin: 0, fontSize: 18, fontWeight: 600 }}>{i18next.t("errorBoundary.title", "Something went wrong")}</h2>
          <p style={{ margin: 0, fontSize: 14, color: "var(--color-text-secondary, #666)" }}>
            {i18next.t("errorBoundary.description", "Please try refreshing the application.")}
          </p>
          <button
            onClick={() => window.location.reload()}
            style={{
              marginTop: 8, padding: "8px 20px", cursor: "pointer",
              backgroundColor: "var(--color-accent, #2563eb)", color: "#fff",
              border: "none", borderRadius: 6, fontSize: 14, fontWeight: 600,
            }}
          >
            {i18next.t("errorBoundary.refresh", "Refresh")}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

export default function App() {
  const [authed, setAuthed] = useState(isAuthenticated());

  useEffect(() => {
    if (authed) {
      consumeOAuthCallbackParams();
    }
  }, [authed]);

  useEffect(() => {
    logStartupTiming("react app mounted");
    const splash = document.getElementById("splash");
    if (!splash) return;
    const splashStart = (window as unknown as Record<string, number>).__splashStart || Date.now();
    const minDisplay = 2200;
    const elapsed = Date.now() - splashStart;
    const remaining = Math.max(0, minDisplay - elapsed);
    setTimeout(() => {
      logStartupTiming("splash fade started");
      splash.classList.add("fade-out");
      setTimeout(() => {
        splash.remove();
        document.getElementById("splash-style")?.remove();
        logStartupTiming("splash removed");
      }, 500);
    }, remaining);
  }, []);

  if (!authed) {
    return (
      <ErrorBoundary>
        <LoginPage onLogin={() => setAuthed(true)} />
      </ErrorBoundary>
    );
  }

  return (
    <ErrorBoundary>
      <Layout />
    </ErrorBoundary>
  );
}
