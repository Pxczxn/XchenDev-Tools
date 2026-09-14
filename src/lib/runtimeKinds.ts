/** 与 Rust RUNTIME_KINDS 顺序一致 */
export const RUNTIME_KIND_IDS = [
  "java",
  "python",
  "node",
  "php",
  "rust",
] as const;

export type RuntimeKindId = (typeof RUNTIME_KIND_IDS)[number];

export function isRuntimeKindEnabled(
  kind: string,
  disabledRuntimeKinds: string[],
): boolean {
  const disabled = new Set(
    disabledRuntimeKinds.map((k) => k.trim().toLowerCase()).filter(Boolean),
  );
  return !disabled.has(kind.toLowerCase());
}

export function enabledRuntimeKindIds(
  disabledRuntimeKinds: string[],
): RuntimeKindId[] {
  return RUNTIME_KIND_IDS.filter((id) =>
    isRuntimeKindEnabled(id, disabledRuntimeKinds),
  );
}
