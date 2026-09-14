import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { getAppSettings, saveAppSettings } from "../ipc/client";
import {
  applyTheme,
  getStoredTheme,
  normalizeTheme,
  resolveTheme,
  toggleTheme as flipTheme,
  type ThemeMode,
} from "../lib/theme";

interface ThemeContextValue {
  theme: ThemeMode;
  setTheme: (theme: ThemeMode) => Promise<void>;
  toggleTheme: () => Promise<void>;
  ready: boolean;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readInitialTheme(): ThemeMode {
  const current = document.documentElement.dataset.theme;
  if (current === "dark" || current === "light") return current;
  return resolveTheme(getStoredTheme());
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeMode>(() => readInitialTheme());
  const [ready, setReady] = useState(false);

  const persistTheme = useCallback(async (next: ThemeMode) => {
    applyTheme(next);
    setThemeState(next);
    try {
      const settings = await getAppSettings();
      if (normalizeTheme(settings.theme) === next) return;
      await saveAppSettings({ ...settings, theme: next });
    } catch {
      /* browser preview or IPC unavailable */
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    getAppSettings()
      .then((settings) => {
        if (cancelled) return;
        const configTheme = normalizeTheme(settings.theme);
        applyTheme(configTheme);
        setThemeState(configTheme);
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const setTheme = useCallback(
    async (next: ThemeMode) => {
      await persistTheme(next);
    },
    [persistTheme],
  );

  const toggleTheme = useCallback(async () => {
    await persistTheme(flipTheme(theme));
  }, [persistTheme, theme]);

  const value = useMemo(
    () => ({ theme, setTheme, toggleTheme, ready }),
    [theme, setTheme, toggleTheme, ready],
  );

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error("useTheme must be used within ThemeProvider");
  }
  return ctx;
}
