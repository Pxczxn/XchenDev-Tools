import { describe, expect, it } from "vitest";
import { formatDisplayPath } from "./formatDisplay";

describe("formatDisplayPath", () => {
  it("converts extended drive paths for display", () => {
    expect(formatDisplayPath("\\\\?\\C:\\demo\\web")).toBe("C:\\demo\\web");
  });

  it("converts extended UNC paths for display", () => {
    expect(formatDisplayPath("\\\\?\\UNC\\server\\share\\demo")).toBe(
      "\\\\server\\share\\demo",
    );
  });

  it("keeps regular paths unchanged", () => {
    expect(formatDisplayPath("C:\\demo\\web")).toBe("C:\\demo\\web");
  });
});
