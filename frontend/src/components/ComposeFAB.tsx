import { PenLine } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useUIStore } from "@/stores/ui.store";
import { useComposeStore } from "@/stores/compose.store";
import { useMailStore } from "@/stores/mail.store";

export default function ComposeFAB() {
  const { t } = useTranslation();
  const activeView = useUIStore((s) => s.activeView);
  const openCompose = useComposeStore((s) => s.openCompose);
  const selectedMessageId = useMailStore((s) => s.selectedMessageId);

  if (activeView === "compose" || selectedMessageId) return null;

  return (
    <button
      onClick={() => openCompose("new")}
      aria-label={t("sidebar.compose", "Compose")}
      title={t("sidebar.compose", "Compose")}
      style={{
        position: "fixed",
        bottom: "48px",
        right: "24px",
        zIndex: 100,
        width: "48px",
        height: "48px",
        borderRadius: "50%",
        border: "none",
        backgroundColor: "var(--color-accent, #2563eb)",
        color: "#fff",
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        boxShadow: "0 4px 12px rgba(0,0,0,0.2)",
        transition: "transform 0.15s ease, box-shadow 0.15s ease",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.transform = "scale(1.08)";
        e.currentTarget.style.boxShadow = "0 6px 20px rgba(0,0,0,0.3)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.transform = "scale(1)";
        e.currentTarget.style.boxShadow = "0 4px 12px rgba(0,0,0,0.2)";
      }}
    >
      <PenLine size={20} />
    </button>
  );
}
