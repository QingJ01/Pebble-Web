import { useEffect, useState } from "react";
import { Check, Copy, Languages, LayoutGrid, MoreHorizontal, Search, ShieldCheck, X } from "lucide-react";
import { useTranslation } from "react-i18next";

interface Props {
  text: string;
  position: { x: number; y: number };
  onTranslate: (text: string, position: { x: number; y: number }) => void;
  onSearch: (text: string) => void;
  onCreateRule: (text: string) => void;
  onAddToKanbanNote: (text: string) => void;
  onClose: () => void;
}

export default function SelectionActionPopover({
  text,
  position,
  onTranslate,
  onSearch,
  onCreateRule,
  onAddToKanbanNote,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const [showSecondaryActions, setShowSecondaryActions] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);

  async function handleCopy() {
    await navigator.clipboard.writeText(text);
    setCopied(true);
  }

  const left = Math.min(position.x, window.innerWidth - 260);
  const top = Math.min(position.y + 10, window.innerHeight - 120);

  return (
    <div
      role="toolbar"
      aria-label={t("selection.actions", "Selected text actions")}
      style={{
        position: "fixed",
        left,
        top,
        display: "flex",
        alignItems: "center",
        gap: "4px",
        padding: "6px",
        borderRadius: "8px",
        border: "1px solid var(--color-border)",
        backgroundColor: "var(--color-bg)",
        boxShadow: "0 4px 12px rgba(0,0,0,0.15)",
        zIndex: 1000,
        color: "var(--color-text-primary)",
      }}
      onClick={(e) => e.stopPropagation()}
    >
      <button
        onClick={handleCopy}
        aria-label={copied
          ? t("selection.copiedSelectedText", "Copied selected text")
          : t("selection.copySelectedText", "Copy selected text")}
        title={copied
          ? t("selection.copiedSelectedText", "Copied selected text")
          : t("selection.copySelectedText", "Copy selected text")}
        style={primaryButtonStyle}
      >
        {copied ? <Check size={14} /> : <Copy size={14} />}
        <span>{copied ? t("common.copied", "Copied") : t("common.copy", "Copy")}</span>
      </button>

      <div style={{ position: "relative" }}>
        <button
          onClick={() => setShowSecondaryActions((value) => !value)}
          aria-expanded={showSecondaryActions}
          aria-label={t("selection.moreActions", "More selected-text actions")}
          title={t("selection.moreActions", "More selected-text actions")}
          style={iconButtonStyle}
        >
          <MoreHorizontal size={16} />
        </button>

        {showSecondaryActions && (
          <div
            role="menu"
            style={{
              position: "absolute",
              top: "calc(100% + 6px)",
              right: 0,
              minWidth: "140px",
              padding: "4px",
              borderRadius: "8px",
              border: "1px solid var(--color-border)",
              backgroundColor: "var(--color-bg)",
              boxShadow: "0 4px 12px rgba(0,0,0,0.15)",
            }}
          >
            <button
              role="menuitem"
              onClick={() => onTranslate(text, position)}
              aria-label={t("selection.translateSelectedText", "Translate selected text")}
              style={menuButtonStyle}
            >
              <Languages size={14} />
              <span>{t("selection.translate", "Translate")}</span>
            </button>
            <button
              role="menuitem"
              onClick={() => onSearch(text)}
              aria-label={t("selection.searchSelectedText", "Search selected text")}
              style={menuButtonStyle}
            >
              <Search size={14} />
              <span>{t("selection.search", "Search")}</span>
            </button>
            <button
              role="menuitem"
              onClick={() => onCreateRule(text)}
              aria-label={t("selection.createRuleFromSelection", "Create rule from selected text")}
              style={menuButtonStyle}
            >
              <ShieldCheck size={14} />
              <span>{t("selection.createRule", "Create rule")}</span>
            </button>
            <button
              role="menuitem"
              onClick={() => onAddToKanbanNote(text)}
              aria-label={t("selection.addToKanbanNote", "Add selected text as kanban note")}
              style={menuButtonStyle}
            >
              <LayoutGrid size={14} />
              <span>{t("selection.addToKanbanNoteLabel", "Add to Kanban note")}</span>
            </button>
          </div>
        )}
      </div>

      <button
        onClick={onClose}
        aria-label={t("common.close", "Close")}
        title={t("common.close", "Close")}
        style={iconButtonStyle}
      >
        <X size={15} />
      </button>
    </div>
  );
}

const primaryButtonStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "5px",
  padding: "5px 9px",
  border: "none",
  borderRadius: "6px",
  backgroundColor: "var(--color-accent)",
  color: "#fff",
  cursor: "pointer",
  fontSize: "12px",
  fontWeight: 600,
};

const iconButtonStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  width: "28px",
  height: "28px",
  padding: 0,
  border: "none",
  borderRadius: "6px",
  backgroundColor: "transparent",
  color: "var(--color-text-secondary)",
  cursor: "pointer",
};

const menuButtonStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "8px",
  width: "100%",
  padding: "7px 8px",
  border: "none",
  borderRadius: "6px",
  backgroundColor: "transparent",
  color: "var(--color-text-primary)",
  cursor: "pointer",
  fontSize: "12px",
  textAlign: "left",
};
