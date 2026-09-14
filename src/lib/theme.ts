export type ThemeMode = "dark" | "light";

export const THEME_STORAGE_KEY = "xchendev-theme";

export function getStoredTheme(): ThemeMode | null {
  try {
    const value = localStorage.getItem(THEME_STORAGE_KEY);
    if (value === "dark" || value === "light") return value;
  } catch {
    /* ignore */
  }
  return null;
}

export function resolveTheme(stored: ThemeMode | null): ThemeMode {
  return stored ?? "dark";
}

export function normalizeTheme(value?: string | null): ThemeMode {
  return value === "light" ? "light" : "dark";
}

export function applyTheme(theme: ThemeMode): void {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    /* ignore */
  }
  void syncNativeWindowTheme(theme);
}

export async function syncNativeWindowTheme(theme: ThemeMode): Promise<void> {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setTheme(theme);
  } catch {
    /* browser preview or permission unavailable */
  }
}

export function initTheme(): ThemeMode {
  const theme = resolveTheme(getStoredTheme());
  applyTheme(theme);
  return theme;
}

export function toggleTheme(current: ThemeMode): ThemeMode {
  return current === "dark" ? "light" : "dark";
}
