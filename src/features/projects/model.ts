import type {
  LaunchProfile,
  LaunchSessionInfo,
  TechnologyCandidate,
} from "../../ipc/types";

export const ACTIVE_SESSION_STATES = new Set(["STARTING", "RUNNING", "STOPPING"]);
export const MAX_PENDING_TERMINAL_EVENTS = 64;
export const MAX_LOG_LINES_PER_SESSION = 2000;

export type SessionsById = Record<string, LaunchSessionInfo>;
export type LogsBySessionId = Record<string, string[]>;
export type LastSessionByProfile = Record<string, string>;
export type WorkspaceView = "scanner" | "runtime";
export type NoticeTone = "info" | "success" | "error";

export interface Notice {
  tone: NoticeTone;
  text: string;
}

export interface TerminalEvent {
  exitCode?: number;
  finalState?: string;
}

export interface ProjectWorkspaceStats {
  technologies: number;
  conflicts: number;
  profiles: number;
  activeSessions: number;
  failedSessions: number;
}

export function indexSessions(sessions: LaunchSessionInfo[]): SessionsById {
  return Object.fromEntries(
    sessions.map((session) => [session.launch_session_id, session]),
  );
}

export function indexLastSessions(
  sessions: LaunchSessionInfo[],
): LastSessionByProfile {
  return Object.fromEntries(
    sessions.map((session) => [session.profile_id, session.launch_session_id]),
  );
}

export function rememberPendingTerminalEvent(
  cache: Record<string, TerminalEvent>,
  sessionId: string,
  event: TerminalEvent,
) {
  if (!(sessionId in cache)) {
    const ids = Object.keys(cache);
    if (ids.length >= MAX_PENDING_TERMINAL_EVENTS) {
      delete cache[ids[0]];
    }
  }
  cache[sessionId] = event;
}

export function appendSessionLog(existing: string[], line: string): string[] {
  if (existing.length < MAX_LOG_LINES_PER_SESSION) {
    return [...existing, line];
  }
  return [
    ...existing.slice(existing.length - MAX_LOG_LINES_PER_SESSION + 1),
    line,
  ];
}

export function activeSessionForProfile(
  sessionsById: SessionsById,
  profileId: string,
): LaunchSessionInfo | undefined {
  return Object.values(sessionsById).find(
    (session) =>
      session.profile_id === profileId && ACTIVE_SESSION_STATES.has(session.state),
  );
}

export function displaySessionForProfile(
  sessionsById: SessionsById,
  lastSessionByProfile: LastSessionByProfile,
  profileId: string,
): LaunchSessionInfo | undefined {
  const active = activeSessionForProfile(sessionsById, profileId);
  if (active) return active;
  const lastSessionId = lastSessionByProfile[profileId];
  return lastSessionId ? sessionsById[lastSessionId] : undefined;
}

export function suggestedRole(
  candidate: TechnologyCandidate,
): "frontend" | "backend" {
  return candidate.stack.toUpperCase() === "NODE" ? "frontend" : "backend";
}

export function terminalState(
  event: TerminalEvent,
  currentState?: string,
): string {
  if (event.finalState) return event.finalState;
  if (currentState === "STOPPING") return "STOPPED";
  if (event.exitCode !== undefined && event.exitCode !== 0) return "FAILED";
  return "STOPPED";
}

export function buildProjectWorkspaceStats(
  candidates: TechnologyCandidate[],
  profiles: LaunchProfile[],
  sessionsById: SessionsById,
  lastSessionByProfile: LastSessionByProfile,
): ProjectWorkspaceStats {
  const displayedSessions = profiles.flatMap((profile) => {
    const session = displaySessionForProfile(
      sessionsById,
      lastSessionByProfile,
      profile.profile_id,
    );
    return session ? [session] : [];
  });

  return {
    technologies: candidates.length,
    conflicts: candidates.filter((candidate) => candidate.status === "CONFLICT").length,
    profiles: profiles.length,
    activeSessions: displayedSessions.filter((session) =>
      ACTIVE_SESSION_STATES.has(session.state),
    ).length,
    failedSessions: displayedSessions.filter(
      (session) => session.state === "FAILED" || (session.exit_code ?? 0) !== 0,
    ).length,
  };
}
