import { describe, expect, it } from "vitest";
import type { LaunchProfile, LaunchSessionInfo } from "../ipc/types";
import { buildProjectRuntimeSnapshot } from "./projectRuntime";

function profile(id: string): LaunchProfile {
  return {
    profile_id: id,
    project_id: "project-1",
    process_role: id,
    working_directory: ".",
    command: `run-${id}`,
    user_modified: false,
  };
}

function session(
  id: string,
  profileId: string,
  state: string,
): LaunchSessionInfo {
  return {
    launch_session_id: id,
    profile_id: profileId,
    state,
  };
}

describe("buildProjectRuntimeSnapshot", () => {
  it("separates active, startable and stoppable project entries", () => {
    const profiles = [profile("frontend"), profile("backend"), profile("worker")];
    const sessions = {
      s1: session("s1", "frontend", "RUNNING"),
      s2: session("s2", "backend", "STOPPING"),
      stale: session("stale", "worker", "STOPPED"),
      other: session("other", "other-project-profile", "RUNNING"),
    };

    const snapshot = buildProjectRuntimeSnapshot(profiles, sessions);

    expect(snapshot.activeSessions.map((item) => item.profile_id)).toEqual([
      "frontend",
      "backend",
    ]);
    expect(snapshot.startableProfiles.map((item) => item.profile_id)).toEqual([
      "worker",
    ]);
    expect(snapshot.stoppableSessions.map((item) => item.profile_id)).toEqual([
      "frontend",
    ]);
  });

  it("treats every profile as startable when the project has no active sessions", () => {
    const profiles = [profile("frontend"), profile("backend")];
    const snapshot = buildProjectRuntimeSnapshot(profiles, {});

    expect(snapshot.activeSessions).toEqual([]);
    expect(snapshot.stoppableSessions).toEqual([]);
    expect(snapshot.startableProfiles).toHaveLength(2);
  });
});
