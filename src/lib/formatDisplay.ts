/** 界面展示用格式化（路径、版本等） */

export function formatDisplayPath(path: string | undefined | null): string {
  if (!path) return "—";
  const uncMatch = path.match(/^\\\\\?\\UNC\\(.*)$/i);
  if (uncMatch) return `\\\\${uncMatch[1]}`;
  return path.replace(/^\\\\\?\\/, "");
}

export function formatRuntimeKind(kind: string): string {
  const map: Record<string, string> = {
    java: "Java",
    python: "Python",
    node: "Node.js",
    php: "PHP",
    rust: "Rust",
  };
  return map[kind.toLowerCase()] ?? kind;
}

/** 压缩版本/探测输出，避免整段错误占满表格 */
export function formatVersionText(version: string | undefined | null): string {
  if (!version) return "—";
  const lines = version
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
    .filter((l) => !l.startsWith("Picked up JAVA_TOOL_OPTIONS"));

  for (const line of lines) {
    const lower = line.toLowerCase();
    if (
      lower.includes("version") ||
      /^v?\d+\.\d+/.test(line) ||
      lower.startsWith("python ") ||
      lower.startsWith("node ") ||
      lower.startsWith("rustc ") ||
      lower.startsWith("cargo ")
    ) {
      return truncate(line, 72);
    }
  }

  const first = lines[0];
  if (!first) return "—";
  if (first.length > 72 || first.toLowerCase().includes("was not found")) {
    return truncate(first, 72);
  }
  return first;
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return `${text.slice(0, max - 1)}…`;
}
