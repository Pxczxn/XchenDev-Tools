import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  THEME_STORAGE_KEY,
  applyTheme,
  getStoredTheme,
  resolveTheme,
  toggleTheme,
} from "./theme";

function createStorage() {
  const store = new Map<string, string>();
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
    removeItem: (key: string) => {
      store.delete(key);
    },
    clear: () => {
      store.clear();
    },
  };
}

const html = { dataset: {} as Record<string, string> };

describe("theme", () => {
  beforeEach(() => {
    html.dataset = {};
    vi.stubGlobal("localStorage", createStorage());
    vi.stubGlobal("document", { documentElement: html });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to dark when nothing stored", () => {
    expect(resolveTheme(getStoredTheme())).toBe("dark");
  });

  it("applies and persists theme", () => {
    applyTheme("light");
    expect(html.dataset.theme).toBe("light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
  });

  it("toggles between dark and light", () => {
    expect(toggleTheme("dark")).toBe("light");
    expect(toggleTheme("light")).toBe("dark");
  });

  it("normalizes invalid theme values to dark", async () => {
    const { normalizeTheme } = await import("./theme");
    expect(normalizeTheme("light")).toBe("light");
    expect(normalizeTheme("dark")).toBe("dark");
    expect(normalizeTheme("system")).toBe("dark");
    expect(normalizeTheme(undefined)).toBe("dark");
  });
});
