import TitleBar from "../components/TitleBar";
import SenderIdentityNotice from "../components/SenderIdentityNotice";
import Sidebar from "../components/Sidebar";
import StatusBar from "../components/StatusBar";
import ComposeFAB from "../components/ComposeFAB";
import InboxView from "../features/inbox/InboxView";
import CommandPalette from "../features/command-palette/CommandPalette";
import ToastContainer from "../components/ToastContainer";
import ConfirmDialog from "../components/ConfirmDialog";
import { useConfirmStore } from "../stores/confirm.store";
import { useComposeStore } from "../stores/compose.store";
import { useUIStore, applyThemeToDom } from "../stores/ui.store";
import { useCommandStore } from "../stores/command.store";
import { useKanbanStore } from "../stores/kanban.store";
import { useKeyboard } from "../hooks/useKeyboard";
import { useNetworkStatus } from "../hooks/useNetworkStatus";
import { buildCommands } from "../features/command-palette/commands";
import { useEffect, lazy, Suspense, Component, type ReactNode, type ErrorInfo } from "react";
import { createLazyViewPreloader, scheduleLazyViewPreload } from "./lazyViewPreload";
import { useRealtimePreferenceSync } from "./useRealtimePreferenceSync";
import { useRealtimeSyncTriggers } from "./useRealtimeSyncTriggers";
import { useNotificationOpenNavigation } from "./useNotificationOpenNavigation";
import { useMailtoOpen } from "./useMailtoOpen";
import AppBackground from "./AppBackground";

const loadSettingsView = () => import("../features/settings/SettingsView");
const loadComposeView = () => import("../features/compose/ComposeView");
const loadKanbanView = () => import("../features/kanban/KanbanView");
const loadContactsView = () => import("../features/contacts/ContactsView");
const loadSearchView = () => import("../features/search/SearchView");
const loadSnoozedView = () => import("../features/snoozed/SnoozedView");
const loadStarredView = () => import("../features/starred/StarredView");
const preloadLazyViews = createLazyViewPreloader([
  loadSettingsView,
  loadComposeView,
  loadKanbanView,
  loadContactsView,
  loadSearchView,
  loadSnoozedView,
  loadStarredView,
]);

const SettingsView = lazy(loadSettingsView);
const ComposeView = lazy(loadComposeView);
const KanbanView = lazy(loadKanbanView);
const ContactsView = lazy(loadContactsView);
const SearchView = lazy(loadSearchView);
const SnoozedView = lazy(loadSnoozedView);
const StarredView = lazy(loadStarredView);
import { useTranslation } from "react-i18next";
import i18next from "i18next";
import { WifiOff } from "lucide-react";
import { listen } from "@/lib/web-events";
import { useQueryClient } from "@tanstack/react-query";

export default function Layout() {
  const activeView = useUIStore((s) => s.activeView);
  const displayedView = activeView;
  const composeKey = useComposeStore((s) => s.composeKey);
  const theme = useUIStore((s) => s.theme);
  const backgroundImage = useUIStore((s) => s.backgroundImage);
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  useKeyboard();

  // Load kanban cards at startup so MessageItem can show kanban indicators
  useEffect(() => {
    useKanbanStore.getState().fetchCards();
  }, []);

  useEffect(() => scheduleLazyViewPreload(preloadLazyViews), []);

  useNetworkStatus();
  useRealtimePreferenceSync();
  useRealtimeSyncTriggers();
  useNotificationOpenNavigation();
  useMailtoOpen();

  // Re-register commands when language changes
  useEffect(() => {
    useCommandStore.getState().registerCommands(buildCommands(t));
  }, [t]);

  // Global listener: refresh data when snoozed messages are restored
  useEffect(() => {
    const unlisten = listen<{ message_id: string; return_to?: string }>("mail:unsnoozed", () => {
      queryClient.invalidateQueries({ queryKey: ["messages"] });
      queryClient.invalidateQueries({ queryKey: ["snoozed"] });

      queryClient.invalidateQueries({ queryKey: ["kanban"] });
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [queryClient]);

  useEffect(() => {
    applyThemeToDom(theme);
    if (theme === "system") {
      const mql = window.matchMedia("(prefers-color-scheme: dark)");
      const listener = () => {
        applyThemeToDom("system");
      };
      mql.addEventListener("change", listener);
      return () => mql.removeEventListener("change", listener);
    }
  }, [theme]);

  return (
    <div
      className={`app-shell flex flex-col h-screen overflow-hidden${backgroundImage ? " app-shell--with-background" : ""}`}
    >
      <AppBackground image={backgroundImage} />
      <TitleBar />
      <SenderIdentityNotice />
      <div className="flex flex-1 min-h-0 app-shell-content">
        <Sidebar />
        <main className="flex-1 min-w-0 overflow-auto scroll-region app-main-scroll" style={{ position: "relative" }}>
          <OfflineBanner />
          <ViewErrorBoundary key={displayedView}>
            <Suspense fallback={<ViewLoadingFallback />}>
              {displayedView === "inbox" && <InboxView />}
              {displayedView === "kanban" && <KanbanView />}
              {displayedView === "contacts" && <ContactsView />}
              {displayedView === "settings" && <SettingsView />}
              {displayedView === "search" && <SearchView />}
              {displayedView === "snoozed" && <SnoozedView />}
              {displayedView === "starred" && <StarredView />}
              {displayedView === "compose" && <ComposeView key={composeKey} />}
            </Suspense>
          </ViewErrorBoundary>
        </main>
      </div>
      <ComposeFAB />
      <StatusBar />
      <CommandPalette />
      <ToastContainer />
      <GlobalConfirmDialog />
    </div>
  );
}

function ViewLoadingFallback() {
  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      height: "100%",
      color: "var(--color-text-secondary)",
      fontSize: "13px",
    }}>
      {i18next.t("common.loading", "Loading...")}
    </div>
  );
}

class ViewErrorBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[ViewError]", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div style={{
          display: "flex", flexDirection: "column", alignItems: "center",
          justifyContent: "center", height: "100%", gap: 12, padding: 24,
          color: "var(--color-text-secondary)",
        }}>
          <p style={{ fontSize: 14, margin: 0 }}>{i18next.t("errorBoundary.title", "Something went wrong")}</p>
          <p style={{ fontSize: 12, margin: 0, color: "var(--color-text-secondary)" }}>
            {i18next.t("errorBoundary.description", "Please try again or refresh the application.")}
          </p>
          {this.state.error && import.meta.env.DEV && (
            <pre style={{ fontSize: 11, color: "#ef4444", maxWidth: "90%", overflow: "auto", whiteSpace: "pre-wrap", textAlign: "left" }}>
              {this.state.error.message}
              {"\n"}
              {this.state.error.stack}
            </pre>
          )}
          <button
            onClick={() => this.setState({ error: null })}
            style={{
              padding: "6px 16px", cursor: "pointer",
              backgroundColor: "var(--color-accent)", color: "#fff",
              border: "none", borderRadius: 6, fontSize: 13,
            }}
          >
            {i18next.t("errorBoundary.retry", "Retry")}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

function GlobalConfirmDialog() {
  const { isOpen, title, message, destructive, confirmLabel, cancelLabel, handleConfirm, handleCancel } = useConfirmStore();
  if (!isOpen) return null;
  return (
    <ConfirmDialog
      title={title}
      message={message}
      destructive={destructive}
      confirmLabel={confirmLabel}
      cancelLabel={cancelLabel}
      onConfirm={handleConfirm}
      onCancel={handleCancel}
    />
  );
}

function OfflineBanner() {
  const networkStatus = useUIStore((s) => s.networkStatus);
  if (networkStatus === "online") return null;
  return (
    <div style={{
      display: "flex", alignItems: "center", gap: "8px",
      padding: "6px 16px",
      backgroundColor: "rgba(239,68,68,0.1)",
      borderBottom: "1px solid rgba(239,68,68,0.2)",
      color: "#ef4444", fontSize: "12px",
    }}>
      <WifiOff size={14} />
      {i18next.t("status.offline", "Offline")} — {i18next.t("status.offlineHint", "Mail sync is paused until you're back online")}
    </div>
  );
}
