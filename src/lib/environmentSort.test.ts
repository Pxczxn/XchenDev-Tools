import { describe, expect, it } from "vitest";
import type { EnvironmentCandidate } from "../ipc/types";
import {
  sortEnvironmentCandidates,
  visibleEnvironmentCandidates,
} from "./environmentSort";

describe("visibleEnvironmentCandidates", () => {
  it("hides invalid auto-detected entries but keeps manual override", () => {
    const input: EnvironmentCandidate[] = [
      {
        runtime_kind: "python",
        source: "NATIVE_COMMAND",
        executable_path: "C:\\WindowsApps\\python.exe",
        validation_status: "INVALID",
        is_user_configured: false,
      },
      {
        runtime_kind: "python",
        source: "NATIVE_COMMAND",
        executable_path: "D:\\py\\python.exe",
        validation_status: "VALID",
        is_user_configured: false,
      },
      {
        runtime_kind: "python",
        source: "MANUAL_OVERRIDE",
        executable_path: "E:\\bad\\python.exe",
        validation_status: "INVALID",
        is_user_configured: true,
      },
    ];
    const visible = visibleEnvironmentCandidates(input);
    expect(visible).toHaveLength(2);
    expect(visible.map((c) => c.executable_path)).toEqual([
      "D:\\py\\python.exe",
      "E:\\bad\\python.exe",
    ]);
  });
});

describe("sortEnvironmentCandidates", () => {
  it("orders manual before native before env before registry", () => {
    const input: EnvironmentCandidate[] = [
      {
        runtime_kind: "java",
        source: "REGISTRY",
        executable_path: "C:\\reg\\java.exe",
        validation_status: "VALID",
        is_user_configured: false,
      },
      {
        runtime_kind: "java",
        source: "MANUAL_OVERRIDE",
        executable_path: "D:\\manual\\java.exe",
        validation_status: "VALID",
        is_user_configured: true,
      },
      {
        runtime_kind: "java",
        source: "NATIVE_COMMAND",
        executable_path: "C:\\cmd\\java.exe",
        validation_status: "VALID",
        is_user_configured: false,
      },
      {
        runtime_kind: "java",
        source: "ENVIRONMENT_VARIABLE",
        executable_path: "C:\\env\\java.exe",
        validation_status: "VALID",
        is_user_configured: false,
      },
    ];
    const sorted = sortEnvironmentCandidates(input);
    expect(sorted.map((c) => c.source)).toEqual([
      "MANUAL_OVERRIDE",
      "NATIVE_COMMAND",
      "ENVIRONMENT_VARIABLE",
      "REGISTRY",
    ]);
  });
});
