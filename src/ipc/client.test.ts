import { describe, expect, it, vi, beforeEach } from "vitest";

let failHealth = false;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "health_check") {
      if (failHealth) {
        throw { code: "IPC_NOT_READY", message: "IPC not ready" };
      }
      return {
        app_version: "0.1.0",
        platform: "windows",
        ipc_status: "ONLINE",
      };
    }
    return null;
  }),
}));

import { healthCheck } from "./client";

describe("healthCheck", () => {
  beforeEach(() => {
    failHealth = false;
  });

  it("returns backend payload on success", async () => {
    const data = await healthCheck();
    expect(data.ipc_status).toBe("ONLINE");
    expect(data.app_version).toBe("0.1.0");
  });

  it("surfaces error code on failure", async () => {
    failHealth = true;
    await expect(healthCheck()).rejects.toMatchObject({
      code: "IPC_NOT_READY",
    });
  });
});
