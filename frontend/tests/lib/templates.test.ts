import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/web-commands";
import { deleteTemplate, listTemplates, saveTemplate } from "../../src/lib/templates";

vi.mock("@/lib/web-commands", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("templates secure storage", () => {
  beforeEach(() => {
    localStorage.clear();
    invokeMock.mockReset();
  });

  it("loads templates from backend storage and clears legacy localStorage", async () => {
    localStorage.setItem("pebble-templates", JSON.stringify([{ id: "legacy" }]));
    invokeMock.mockResolvedValue([{ id: "template-1", name: "Intro", subject: "Hello", body: "Body", createdAt: 1 }]);

    const templates = await listTemplates();

    expect(invokeMock).toHaveBeenCalledWith("list_email_templates");
    expect(templates).toHaveLength(1);
    expect(localStorage.getItem("pebble-templates")).toBeNull();
  });

  it("migrates legacy templates that are missing from backend storage", async () => {
    const legacy = {
      id: "legacy-1",
      name: "Legacy reply",
      subject: "Old subject",
      body: "Old body",
      createdAt: 10,
    };
    const secure = { id: "template-1", name: "Intro", subject: "Hello", body: "Body", createdAt: 1 };
    const migrated = { ...legacy, id: "template-2", createdAt: 20 };
    localStorage.setItem("pebble-templates", JSON.stringify([legacy]));
    invokeMock.mockResolvedValueOnce([secure]).mockResolvedValueOnce(migrated);

    const templates = await listTemplates();

    expect(invokeMock).toHaveBeenNthCalledWith(2, "save_email_template", {
      template: {
        name: legacy.name,
        subject: legacy.subject,
        body: legacy.body,
        deduplicateByContent: true,
      },
    });
    expect(templates).toEqual([secure, migrated]);
    expect(localStorage.getItem("pebble-templates")).toBeNull();
  });

  it("migrates duplicate legacy template content only once", async () => {
    const legacy = {
      id: "legacy-1",
      name: "Legacy reply",
      subject: "Old subject",
      body: "Old body",
      createdAt: 10,
    };
    const duplicate = { ...legacy, id: "legacy-2", createdAt: 11 };
    const migrated = { ...legacy, id: "template-1", createdAt: 20 };
    localStorage.setItem("pebble-templates", JSON.stringify([legacy, duplicate]));
    invokeMock.mockResolvedValueOnce([]).mockResolvedValueOnce(migrated);

    await expect(listTemplates()).resolves.toEqual([migrated]);

    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "save_email_template", {
      template: {
        name: legacy.name,
        subject: legacy.subject,
        body: legacy.body,
        deduplicateByContent: true,
      },
    });
  });

  it("keeps legacy templates available when migration cannot be persisted", async () => {
    const legacy = {
      id: "legacy-1",
      name: "Legacy reply",
      subject: "Old subject",
      body: "Old body",
      createdAt: 10,
    };
    localStorage.setItem("pebble-templates", JSON.stringify([legacy]));
    invokeMock.mockResolvedValueOnce([]).mockRejectedValueOnce(new Error("write failed"));

    await expect(listTemplates()).resolves.toEqual([legacy]);

    expect(localStorage.getItem("pebble-templates")).toBe(JSON.stringify([legacy]));
  });

  it("saves and deletes templates through backend storage without writing localStorage", async () => {
    invokeMock
      .mockResolvedValueOnce({ id: "template-1", name: "Intro", subject: "Hello", body: "Body", createdAt: 1 })
      .mockResolvedValueOnce(undefined);

    await saveTemplate({ name: "Intro", subject: "Hello", body: "Body" });
    await deleteTemplate("template-1");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "save_email_template", {
      template: { name: "Intro", subject: "Hello", body: "Body" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "delete_email_template", { id: "template-1" });
    expect(localStorage.getItem("pebble-templates")).toBeNull();
  });
});
