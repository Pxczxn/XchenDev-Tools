export const MANAGED_SERVICE_KIND_IDS = ["mysql", "redis"] as const;

export type ManagedServiceKindId = (typeof MANAGED_SERVICE_KIND_IDS)[number];

export const DEFAULT_MANAGED_SERVICE_KINDS: ManagedServiceKindId[] = [
  "mysql",
  "redis",
];

export function formatManagedServiceKind(kind: string): string {
  const map: Record<string, string> = {
    mysql: "MySQL",
    redis: "Redis",
  };
  return map[kind.toLowerCase()] ?? kind;
}

/** 过滤非法 id；`kinds` 为 undefined 时使用默认 MySQL + Redis */
export function normalizeManagedServiceKinds(
  kinds: string[] | undefined,
): ManagedServiceKindId[] {
  const source =
    kinds === undefined ? [...DEFAULT_MANAGED_SERVICE_KINDS] : kinds;
  const allowed = new Set<string>(MANAGED_SERVICE_KIND_IDS);
  const out = source
    .map((k) => k.trim().toLowerCase())
    .filter((k) => allowed.has(k));
  return [...new Set(out)] as ManagedServiceKindId[];
}

export function sanitizeManagedServiceKinds(
  kinds: string[],
): ManagedServiceKindId[] {
  const allowed = new Set<string>(MANAGED_SERVICE_KIND_IDS);
  const out = kinds
    .map((k) => k.trim().toLowerCase())
    .filter((k) => allowed.has(k));
  return [...new Set(out)] as ManagedServiceKindId[];
}

export function managedServicesSummary(kinds: ManagedServiceKindId[]): string {
  if (kinds.length === 0) return "（未选择）";
  return kinds.map(formatManagedServiceKind).join(" / ");
}

/** IPC 返回的 kind 为 SCREAMING_SNAKE_CASE，如 MYSQL */
export function serviceKindToId(kind: string): ManagedServiceKindId | null {
  const key = kind.trim().toUpperCase();
  if (key === "MYSQL") return "mysql";
  if (key === "REDIS") return "redis";
  return null;
}

export function missingManagedServiceKinds(
  enabled: ManagedServiceKindId[],
  services: { kind: string }[],
): ManagedServiceKindId[] {
  const found = new Set(
    services
      .map((s) => serviceKindToId(s.kind))
      .filter((k): k is ManagedServiceKindId => k !== null),
  );
  return enabled.filter((id) => !found.has(id));
}
