import type { EnvironmentCandidate } from "../ipc/types";

const SOURCE_ORDER: Record<string, number> = {
  MANUAL_OVERRIDE: 0,
  NATIVE_COMMAND: 1,
  ENVIRONMENT_VARIABLE: 2,
  REGISTRY: 3,
};

const VALIDATION_ORDER: Record<string, number> = {
  VALID: 0,
  INVALID: 1,
  UNKNOWN: 2,
};

function sourceRank(source: string): number {
  return SOURCE_ORDER[source.toUpperCase()] ?? 99;
}

function validationRank(status: string): number {
  return VALIDATION_ORDER[status.toUpperCase()] ?? 99;
}

function candidatePath(candidate: EnvironmentCandidate): string {
  return (
    candidate.resolved_path ??
    candidate.executable_path ??
    ""
  ).toLowerCase();
}

export function isVisibleEnvironmentCandidate(
  candidate: EnvironmentCandidate,
): boolean {
  return (
    candidate.validation_status.toUpperCase() !== "INVALID" ||
    candidate.is_user_configured
  );
}

export function visibleEnvironmentCandidates(
  candidates: EnvironmentCandidate[],
): EnvironmentCandidate[] {
  return candidates.filter(isVisibleEnvironmentCandidate);
}

export function sortEnvironmentCandidates(
  candidates: EnvironmentCandidate[],
): EnvironmentCandidate[] {
  return [...candidates].sort((a, b) => {
    const bySource = sourceRank(a.source) - sourceRank(b.source);
    if (bySource !== 0) return bySource;
    const byValid =
      validationRank(a.validation_status) - validationRank(b.validation_status);
    if (byValid !== 0) return byValid;
    return candidatePath(a).localeCompare(candidatePath(b));
  });
}
