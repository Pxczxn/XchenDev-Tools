import type { LaunchProfile, LaunchSessionInfo } from "../ipc/types";

const ACTIVE_SESSION_STATES = new Set(["STARTING", "RUNNING", "STOPPING"]);

export interface ProjectRuntimeSnapshot {
  activeSessions: LaunchSessionInfo[];
  startableProfiles: LaunchProfile[];
  stoppableSessions: LaunchSessionInfo[];
}

export function buildProjectRuntimeSnapshot(
  profiles: LaunchProfile[],
  sessionsById: Record<string, LaunchSessionInfo>,
): ProjectRuntimeSnapshot {
  const activeByProfile = new Map<string, LaunchSessionInfo>();

  for (const session of Object.values(sessionsById)) {
    if (!ACTIVE_SESSION_STATES.has(session.state)) continue;
    if (!activeByProfile.has(session.profile_id)) {
      activeByProfile.set(session.profile_id, session);
    }
  }

  const activeSessions = profiles.flatMap((profile) => {
    const session = activeByProfile.get(profile.profile_id);
    return session ? [session] : [];
  });
  const startableProfiles = profiles.filter(
    (profile) => !activeByProfile.has(profile.profile_id),
  );
  const stoppableSessions = activeSessions.filter(
    (session) => session.state !== "STOPPING",
  );

  return { activeSessions, startableProfiles, stoppableSessions };
}
