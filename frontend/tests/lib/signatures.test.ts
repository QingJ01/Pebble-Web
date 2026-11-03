import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/web-commands";
import { getSignature, setSignature } from "../../src/lib/signatures";

vi.mock("@/lib/web-commands", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

describe("signatures secure storage", () => {
  beforeEach(() => {
    localStorage.clear();
    invokeMock.mockReset();
  });

  it("loads signatures from backend storage and clears legacy localStorage", async () => {
    localStorage.setItem("pebble-signatures", JSON.stringify({ "account-1": "legacy" }));
    invokeMock.mockResolvedValue("secure signature");

    const signature = await getSignature("account-1");

    expect(invokeMock).toHaveBeenCalledWith("get_email_signature", { accountId: "account-1" });
    expect(signature).toBe("secure signature");
    expect(localStorage.getItem("pebble-signatures")).toBeNull();
  });

  it("migrates a legacy signature when backend storage is empty", async () => {
    localStorage.setItem("pebble-signatures", JSON.stringify({
      "account-1": "legacy signature",
      "account-2": "keep me",
    }));
    invokeMock.mockResolvedValueOnce("").mockResolvedValueOnce("legacy signature");

    const signature = await getSignature("account-1");

    expect(signature).toBe("legacy signature");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "migrate_email_signature_if_absent", {
      accountId: "account-1",
      signature: "legacy signature",
    });
    expect(JSON.parse(localStorage.getItem("pebble-signatures") ?? "{}"))
      .toEqual({ "account-2": "keep me" });
  });

  it("does not let delayed legacy migration overwrite a concurrent save", async () => {
    localStorage.setItem("pebble-signatures", JSON.stringify({ "account-1": "legacy" }));
    const delayedGet = deferred<string>();
    let storedSignature = "";
    invokeMock.mockImplementation((command, args) => {
      if (command === "get_email_signature") return delayedGet.promise;
      if (command === "set_email_signature") {
        storedSignature = (args as { signature: string }).signature;
        return Promise.resolve(undefined);
      }
      if (command === "migrate_email_signature_if_absent") {
        if (!storedSignature) storedSignature = (args as { signature: string }).signature;
        return Promise.resolve(storedSignature);
      }
      return Promise.resolve(undefined);
    });

    const delayedLegacyRead = getSignature("account-1");
    await setSignature("account-1", "new signature");
    delayedGet.resolve("");

    await expect(delayedLegacyRead).resolves.toBe("new signature");
    expect(storedSignature).toBe("new signature");
  });

  it("does not restore legacy data after a concurrent explicit clear", async () => {
    localStorage.setItem("pebble-signatures", JSON.stringify({ "account-1": "legacy" }));
    const delayedGet = deferred<string>();
    let storedSignature: string | undefined;
    invokeMock.mockImplementation((command, args) => {
      if (command === "get_email_signature") return delayedGet.promise;
      if (command === "set_email_signature") {
        storedSignature = (args as { signature: string }).signature;
        return Promise.resolve(undefined);
      }
      if (command === "migrate_email_signature_if_absent") {
        if (storedSignature === undefined) {
          storedSignature = (args as { signature: string }).signature;
        }
        return Promise.resolve(storedSignature);
      }
      return Promise.resolve(undefined);
    });

    const delayedLegacyRead = getSignature("account-1");
    await setSignature("account-1", "");
    delayedGet.resolve("");

    await expect(delayedLegacyRead).resolves.toBe("");
    expect(storedSignature).toBe("");
    expect(localStorage.getItem("pebble-signatures")).toBeNull();
  });

  it("keeps a legacy signature when migration cannot be persisted", async () => {
    localStorage.setItem("pebble-signatures", JSON.stringify({ "account-1": "legacy signature" }));
    invokeMock.mockResolvedValueOnce("").mockRejectedValueOnce(new Error("write failed"));

    await expect(getSignature("account-1")).resolves.toBe("legacy signature");

    expect(localStorage.getItem("pebble-signatures"))
      .toBe(JSON.stringify({ "account-1": "legacy signature" }));
  });

  it("saves signatures through backend storage without writing localStorage", async () => {
    invokeMock.mockResolvedValue(undefined);

    await setSignature("account-1", "Regards");

    expect(invokeMock).toHaveBeenCalledWith("set_email_signature", {
      accountId: "account-1",
      signature: "Regards",
    });
    expect(localStorage.getItem("pebble-signatures")).toBeNull();
  });

  it("does not delete another account's legacy signature after saving", async () => {
    localStorage.setItem("pebble-signatures", JSON.stringify({
      "account-1": "old",
      "account-2": "keep me",
    }));
    invokeMock.mockResolvedValue(undefined);

    await setSignature("account-1", "new");

    expect(JSON.parse(localStorage.getItem("pebble-signatures") ?? "{}"))
      .toEqual({ "account-2": "keep me" });
  });
});
