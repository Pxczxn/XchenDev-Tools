import { describe, expect, it } from "vitest";
import { enabledRuntimeKindIds, isRuntimeKindEnabled } from "./runtimeKinds";

describe("runtimeKinds", () => {
  it("treats disabled list case-insensitively", () => {
    expect(isRuntimeKindEnabled("php", ["PHP"])).toBe(false);
    expect(isRuntimeKindEnabled("node", ["php"])).toBe(true);
  });

  it("enabledRuntimeKindIds omits disabled", () => {
    const ids = enabledRuntimeKindIds(["php", "java"]);
    expect(ids).not.toContain("php");
    expect(ids).not.toContain("java");
    expect(ids).toContain("node");
  });
});
