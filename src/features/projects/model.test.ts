import { describe, expect, it } from "vitest";
import type {
  LaunchProfile,
  LaunchSessionInfo,
  TechnologyCandidate,
} from "../../ipc/types";
import {
  appendSessionLog,
  buildProjectWorkspaceStats,
  displaySessionForProfile,
  terminalState,
} from "./model";

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
  exitCode?: number,
): LaunchSessionInfo {
  return {
    launch_session_id: id,
    profile_id: profileId,
    state,
    exit_code: exitCode,
  };
}

function candidate(id: string, status = "READY"): TechnologyCandidate {
  return {
    id,
    directory: ".",
    evidence_file: "package.json",
    stack: "NODE",
    status,
  };
}

describe("project manager model", () => {
  it("prefers active session over remembered terminal session", () => {
    const sessions = {
      old: session("old", "frontend", "STOPPED", 0),
      live: session("live", "frontend", "RUNNING"),
    };

    expect(
      displaySessionForProfile(sessions, { frontend: "old" }, "frontend")
        ?.launch_session_id,
    ).toBe("live");
  });

  it("builds workspace stats from current project context", () => {
    const profiles = [profile("frontend"), profile("backend")];
    const sessions = {
      live: session("live", "frontend", "RUNNING"),
      failed: session("failed", "backend", "FAILED", 1),
      unrelated: session("unrelated", "other", "RUNNING"),
    };

    expect(
      buildProjectWorkspaceStats(
        [candidate("node"), candidate("maven", "CONFLICT")],
        profiles,
        sessions,
        { frontend: "live", backend: "failed", other: "unrelated" },
      ),
    ).toEqual({
      technologies: 2,
      conflicts: 1,
      profiles: 2,
      activeSessions: 1,
      failedSessions: 1,
    });
  });

  it("maps terminal events without losing stop intent", () => {
    expect(terminalState({ exitCode: 1 }, "STOPPING")).toBe("STOPPED");
    expect(terminalState({ exitCode: 1 }, "RUNNING")).toBe("FAILED");
    expect(terminalState({ finalState: "FAILED", exitCode: 1 })).toBe(
      "FAILED",
    );
  });

  it("keeps appended output ordered", () => {
    expect(appendSessionLog(["a", "b"], "c")).toEqual(["a", "b", "c"]);
  });
});
