import { useTheme } from "../context/ThemeContext";

export function ThemeToggle() {
  const { theme, toggleTheme } = useTheme();

  return (
    <button
      type="button"
      className="theme-toggle"
      onClick={() => void toggleTheme()}
      aria-label={theme === "dark" ? "切换到浅色主题" : "切换到深色主题"}
      title={theme === "dark" ? "切换到浅色主题" : "切换到深色主题"}
    >
      <span className="theme-toggle-icon" aria-hidden>
        {theme === "dark" ? "☀" : "☾"}
      </span>
      <span>{theme === "dark" ? "浅色" : "深色"}</span>
    </button>
  );
}
