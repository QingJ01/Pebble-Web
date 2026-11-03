import { invoke } from "@/lib/web-commands";

const LEGACY_STORAGE_KEY = "pebble-templates";

export interface EmailTemplate {
  id: string;
  name: string;
  subject: string;
  body: string;
  createdAt: number;
}

function clearLegacyTemplates() {
  try {
    localStorage.removeItem(LEGACY_STORAGE_KEY);
  } catch { /* ignored */ }
}

function isEmailTemplate(value: unknown): value is EmailTemplate {
  if (!value || typeof value !== "object") return false;
  const template = value as Partial<EmailTemplate>;
  return typeof template.id === "string"
    && typeof template.name === "string"
    && typeof template.subject === "string"
    && typeof template.body === "string"
    && typeof template.createdAt === "number";
}

function readLegacyTemplates(): EmailTemplate[] | null {
  try {
    const raw = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) && parsed.every(isEmailTemplate) ? parsed : [];
  } catch {
    return [];
  }
}

function writeLegacyTemplates(templates: EmailTemplate[]) {
  try {
    if (templates.length === 0) {
      localStorage.removeItem(LEGACY_STORAGE_KEY);
    } else {
      localStorage.setItem(LEGACY_STORAGE_KEY, JSON.stringify(templates));
    }
  } catch { /* ignored */ }
}

function sameTemplateContent(
  left: Pick<EmailTemplate, "name" | "subject" | "body">,
  right: Pick<EmailTemplate, "name" | "subject" | "body">,
) {
  return left.name === right.name && left.subject === right.subject && left.body === right.body;
}

export async function listTemplates(): Promise<EmailTemplate[]> {
  const templates = await invoke<EmailTemplate[]>("list_email_templates");
  const legacyTemplates = readLegacyTemplates();
  if (legacyTemplates === null) return templates;
  if (legacyTemplates.length === 0) {
    clearLegacyTemplates();
    return templates;
  }

  const migrated = [...templates];
  const failed: EmailTemplate[] = [];
  for (const legacy of legacyTemplates) {
    const alreadyStored = migrated.some((template) => (
      template.id === legacy.id || sameTemplateContent(template, legacy)
    ));
    if (alreadyStored) continue;

    try {
      const saved = await invoke<EmailTemplate>("save_email_template", {
        template: {
          name: legacy.name,
          subject: legacy.subject,
          body: legacy.body,
          deduplicateByContent: true,
        },
      });
      if (!migrated.some((template) => (
        template.id === saved.id || sameTemplateContent(template, saved)
      ))) {
        migrated.push(saved);
      }
    } catch (error) {
      console.warn("Failed to migrate legacy email template:", error);
      failed.push(legacy);
      migrated.push(legacy);
    }
  }

  writeLegacyTemplates(failed);
  return migrated;
}

export async function saveTemplate(template: Omit<EmailTemplate, "id" | "createdAt">): Promise<EmailTemplate> {
  const saved = await invoke<EmailTemplate>("save_email_template", { template });
  const legacyTemplates = readLegacyTemplates();
  if (legacyTemplates) {
    writeLegacyTemplates(legacyTemplates.filter((legacy) => !sameTemplateContent(legacy, template)));
  }
  return saved;
}

export async function deleteTemplate(id: string): Promise<void> {
  await invoke<void>("delete_email_template", { id });
  const legacyTemplates = readLegacyTemplates();
  if (legacyTemplates) {
    writeLegacyTemplates(legacyTemplates.filter((legacy) => legacy.id !== id));
  }
}
