import { describe, expect, it } from "vitest";
import { labelErrorText, labelStatus } from "./statusLabels";

describe("statusLabels", () => {
  it("maps IPC status", () => {
    expect(labelStatus("ONLINE")).toBe("在线");
  });

  it("maps error codes with detail", () => {
    expect(labelErrorText("PATH_NOT_FOUND:路径不存在")).toBe("路径不存在");
  });

  it("maps disabled runtime errors", () => {
    expect(labelErrorText("RUNTIME_DISABLED:运行时 node 已在设置中禁用")).toBe(
      "运行时已禁用：运行时 node 已在设置中禁用",
    );
  });

  it("maps validation status", () => {
    expect(labelStatus("VALID")).toBe("可用");
  });
});
