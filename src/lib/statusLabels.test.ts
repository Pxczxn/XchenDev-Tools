import { describe, expect, it } from "vitest";
import { labelErrorText, labelStatus } from "./statusLabels";

describe("statusLabels", () => {
  it("maps IPC status", () => {
    expect(labelStatus("ONLINE")).toBe("在线");
  });

  it("maps error codes with detail", () => {
    expect(labelErrorText("PATH_NOT_FOUND:路径不存在")).toBe("路径不存在");
  });

  it("maps validation status", () => {
    expect(labelStatus("VALID")).toBe("可用");
  });
});
