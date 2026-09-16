import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  issueLaunchConfirmation,
  listLaunchProfiles,
  listLaunchSessions,
  listProjects,
  removeLaunchProfile,
  removeProject,
  saveLaunchProfile,
  scanProjectDirectory,
  startLaunchProfile,
  stopLaunchSession,
  upsertProject,
} from "../../ipc/client";
import type {
  LaunchProfile,
  LaunchSessionInfo,
  ProjectInfo,
  TechnologyCandidate,
} from "../../ipc/types";
import { buildProjectRuntimeSnapshot } from "../../lib/projectRuntime";
import { labelErrorText, labelStatus } from "../../lib/statusLabels";
import {
  ACTIVE_SESSION_STATES,
  activeSessionForProfile,
  appendSessionLog,
  buildProjectWorkspaceStats,
  indexLastSessions,
  indexSessions,
  rememberPendingTerminalEvent,
  suggestedRole,
  terminalState,
  type LastSessionByProfile,
  type LogsBySessionId,
  type Notice,
  type SessionsById,
  type TerminalEvent,
  type WorkspaceView,
} from "./model";

export function useProjectManager() {
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [rootPath, setRootPath] = useState("");
  const [projectId, setProjectId] = useState("");
  const [candidates, setCandidates] = useState<TechnologyCandidate[]>([]);
  const [profiles, setProfiles] = useState<LaunchProfile[]>([]);
  const [selectedCandidateId, setSelectedCandidateId] = useState("");
  const [role, setRole] = useState<"frontend" | "backend">("frontend");
  const [command, setCommand] = useState("");
  const [workdir, setWorkdir] = useState("");
  const [sessionsById, setSessionsById] = useState<SessionsById>({});
  const [lastSessionByProfile, setLastSessionByProfile] =
    useState<LastSessionByProfile>({});
  const [logsBySessionId, setLogsBySessionId] = useState<LogsBySessionId>({});
  const [loading, setLoading] = useState(false);
  const [bulkAction, setBulkAction] = useState<"start" | "stop" | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [view, setView] = useState<WorkspaceView>("overview");

  const terminalEventsRef = useRef<Record<string, TerminalEvent>>({});
  const lastSessionByProfileRef = useRef<LastSessionByProfile>({});
  const projectContextGenerationRef = useRef(0);

  const currentProject = useMemo(
    () => projects.find((project) => project.project_id === projectId),
    [projectId, projects],
  );
  const selectedCandidate = useMemo(
    () => candidates.find((candidate) => candidate.id === selectedCandidateId) ?? null,
    [candidates, selectedCandidateId],
  );
  const runtimeSnapshot = useMemo(
    () => buildProjectRuntimeSnapshot(profiles, sessionsById),
    [profiles, sessionsById],
  );
  const stats = useMemo(
    () =>
      buildProjectWorkspaceStats(
        candidates,
        profiles,
        sessionsById,
        lastSessionByProfile,
      ),
    [candidates, profiles, sessionsById, lastSessionByProfile],
  );

  function invalidateProjectContext() {
    projectContextGenerationRef.current += 1;
    setLoading(false);
  }

  function clearComposer() {
    setSelectedCandidateId("");
    setRole("frontend");
    setCommand("");
    setWorkdir("");
  }

  function resetWorkspace(nextRoot = "") {
    invalidateProjectContext();
    setRootPath(nextRoot);
    setProjectId("");
    setCandidates([]);
    setProfiles([]);
    clearComposer();
  }

  function showError(error: unknown) {
    setNotice({ tone: "error", text: labelErrorText(String(error)) });
  }

  useEffect(() => {
    let disposed = false;

    void Promise.all([listProjects(), listLaunchSessions()])
      .then(([savedProjects, sessions]) => {
        if (disposed) return;
        const indexedLastSessions = indexLastSessions(sessions);
        setProjects(savedProjects);
        setSessionsById(indexSessions(sessions));
        lastSessionByProfileRef.current = indexedLastSessions;
        setLastSessionByProfile(indexedLastSessions);
      })
      .catch((error) => {
        if (!disposed) showError(error);
      });

    const unlisten = listen<{
      launchSessionId: string;
      stream: string;
      chunk: string;
      final?: boolean;
      exitCode?: number;
      finalState?: string;
    }>("launch_output", (event) => {
      const payload = event.payload;
      const line = payload.final
        ? `[${labelStatus("exit")}] 退出码=${payload.exitCode ?? "?"}`
        : `[${labelStatus(payload.stream)}] ${payload.chunk}`;

      setLogsBySessionId((previous) => ({
        ...previous,
        [payload.launchSessionId]: appendSessionLog(
          previous[payload.launchSessionId] ?? [],
          line,
        ),
      }));

      if (!payload.final) return;

      const terminalEvent: TerminalEvent = {
        exitCode: payload.exitCode,
        finalState: payload.finalState,
      };
      rememberPendingTerminalEvent(
        terminalEventsRef.current,
        payload.launchSessionId,
        terminalEvent,
      );

      setSessionsById((previous) => {
        const current = previous[payload.launchSessionId];
        if (!current) return previous;
        return {
          ...previous,
          [payload.launchSessionId]: {
            ...current,
            state: terminalState(terminalEvent, current.state),
            exit_code: payload.exitCode ?? current.exit_code,
          },
        };
      });

      void listLaunchSessions()
        .then((sessions) => {
          if (disposed) return;
          const activeLastSessions = indexLastSessions(sessions);
          setSessionsById((previous) => ({
            ...previous,
            ...indexSessions(sessions),
          }));
          if (Object.keys(activeLastSessions).length === 0) return;
          const nextLastSessions = {
            ...lastSessionByProfileRef.current,
            ...activeLastSessions,
          };
          lastSessionByProfileRef.current = nextLastSessions;
          setLastSessionByProfile(nextLastSessions);
        })
        .catch(() => {
          // Event-derived state is authoritative enough until the next refresh.
        });
    });

    return () => {
      disposed = true;
      void unlisten.then((fn) => fn());
    };
  }, []);

  async function pickDirectory() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    resetWorkspace(picked);
    setView("scanner");
    setNotice({ tone: "info", text: "目录已选择，扫描后会加入项目库" });
  }

  function editRootPath(value: string) {
    resetWorkspace(value);
    setView("scanner");
    setNotice(null);
  }

  async function openProject(project: ProjectInfo) {
    const generation = projectContextGenerationRef.current + 1;
    projectContextGenerationRef.current = generation;
    setLoading(false);
    setRootPath(project.root_path);
    setProjectId(project.project_id);
    setCandidates([]);
    setProfiles([]);
    clearComposer();
    setView("overview");
    setNotice(null);

    try {
      const nextProfiles = await listLaunchProfiles(project.project_id);
      if (generation !== projectContextGenerationRef.current) return;
      setProfiles(nextProfiles);
      setNotice({ tone: "success", text: `已打开项目：${project.name}` });
    } catch (error) {
      if (generation === projectContextGenerationRef.current) showError(error);
    }
  }

  async function removeSavedProject(project: ProjectInfo) {
    if (
      !window.confirm(
        `移除项目记录及其启动配置，不删除磁盘文件。\n运行中的项目需要先停止。\n\n${project.name}`,
      )
    ) {
      return;
    }

    const generation = projectContextGenerationRef.current;
    const removingCurrentProject = projectId === project.project_id;
    try {
      await removeProject(project.project_id);
      const savedProjects = await listProjects();
      setProjects(savedProjects);
      if (generation !== projectContextGenerationRef.current) return;

      if (removingCurrentProject) {
        resetWorkspace();
        setView("overview");
      }
      setNotice({
        tone: "success",
        text: "项目记录与启动配置已移除，磁盘文件未删除",
      });
    } catch (error) {
      if (generation === projectContextGenerationRef.current) showError(error);
    }
  }

  async function scanProject() {
    if (!rootPath) return;
    const generation = projectContextGenerationRef.current + 1;
    projectContextGenerationRef.current = generation;
    const requestedRoot = rootPath;
    setLoading(true);
    setNotice(null);

    try {
      const result = await scanProjectDirectory(requestedRoot);
      if (generation !== projectContextGenerationRef.current) return;
      const project = await upsertProject(result.root_path);
      if (generation !== projectContextGenerationRef.current) return;
      const [nextProfiles, savedProjects] = await Promise.all([
        listLaunchProfiles(project.project_id),
        listProjects(),
      ]);
      if (generation !== projectContextGenerationRef.current) return;

      setRootPath(project.root_path);
      setProjectId(project.project_id);
      setCandidates(result.candidates);
      setProfiles(nextProfiles);
      setProjects(savedProjects);
      clearComposer();
      setView("scanner");
      setNotice({ tone: "success", text: `扫描完成：${project.name}` });
    } catch (error) {
      if (generation === projectContextGenerationRef.current) showError(error);
    } finally {
      if (generation === projectContextGenerationRef.current) setLoading(false);
    }
  }

  function selectCandidate(candidate: TechnologyCandidate) {
    setSelectedCandidateId(candidate.id);
    setRole(suggestedRole(candidate));
    setWorkdir(candidate.directory);
    setCommand(candidate.suggested_command ?? "");
  }

  async function saveProfile() {
    if (!projectId || !workdir.trim() || !command.trim()) return;
    const generation = projectContextGenerationRef.current;
    const targetProjectId = projectId;

    try {
      await saveLaunchProfile({
        projectId: targetProjectId,
        processRole: role,
        workingDirectory: workdir.trim(),
        command: command.trim(),
        sourceCandidateId: selectedCandidate?.id,
      });
      const nextProfiles = await listLaunchProfiles(targetProjectId);
      if (generation !== projectContextGenerationRef.current) return;
      setProfiles(nextProfiles);
      clearComposer();
      setView("runtime");
      setNotice({ tone: "success", text: "启动配置已保存" });
    } catch (error) {
      if (generation === projectContextGenerationRef.current) showError(error);
    }
  }

  async function removeProfile(profile: LaunchProfile) {
    if (!window.confirm(`移除这条启动配置？\n\n${profile.command}`)) return;
    const generation = projectContextGenerationRef.current;

    try {
      await removeLaunchProfile(profile.profile_id);
      const lastSessionId = lastSessionByProfileRef.current[profile.profile_id];
      const nextLastSessions = { ...lastSessionByProfileRef.current };
      delete nextLastSessions[profile.profile_id];
      lastSessionByProfileRef.current = nextLastSessions;
      setLastSessionByProfile(nextLastSessions);

      if (lastSessionId) {
        setSessionsById((previous) => {
          const next = { ...previous };
          delete next[lastSessionId];
          return next;
        });
        setLogsBySessionId((previous) => {
          const next = { ...previous };
          delete next[lastSessionId];
          return next;
        });
        delete terminalEventsRef.current[lastSessionId];
      }

      const nextProfiles = await listLaunchProfiles(profile.project_id);
      if (generation !== projectContextGenerationRef.current) return;
      setProfiles(nextProfiles);
      setNotice({ tone: "success", text: "启动配置已移除" });
    } catch (error) {
      if (generation === projectContextGenerationRef.current) showError(error);
    }
  }

  function rememberStartedSession(
    profile: LaunchProfile,
    info: LaunchSessionInfo,
  ): LaunchSessionInfo {
    const pendingTerminal = terminalEventsRef.current[info.launch_session_id];
    const resolvedInfo = pendingTerminal
      ? {
          ...info,
          state: terminalState(pendingTerminal, info.state),
          exit_code: pendingTerminal.exitCode ?? info.exit_code,
        }
      : info;
    const previousSessionId = lastSessionByProfileRef.current[profile.profile_id];
    const nextLastSessions = {
      ...lastSessionByProfileRef.current,
      [profile.profile_id]: info.launch_session_id,
    };
    lastSessionByProfileRef.current = nextLastSessions;

    setSessionsById((previous) => {
      const next = { ...previous, [info.launch_session_id]: resolvedInfo };
      if (previousSessionId && previousSessionId !== info.launch_session_id) {
        delete next[previousSessionId];
      }
      return next;
    });
    setLastSessionByProfile(nextLastSessions);
    setLogsBySessionId((previous) => {
      const next = {
        ...previous,
        [info.launch_session_id]: previous[info.launch_session_id] ?? [],
      };
      if (previousSessionId && previousSessionId !== info.launch_session_id) {
        delete next[previousSessionId];
      }
      return next;
    });
    if (pendingTerminal) delete terminalEventsRef.current[info.launch_session_id];
    return resolvedInfo;
  }

  function markSessionStopping(session: LaunchSessionInfo) {
    setSessionsById((previous) => {
      const current = previous[session.launch_session_id];
      if (!current || !ACTIVE_SESSION_STATES.has(current.state)) return previous;
      return {
        ...previous,
        [session.launch_session_id]: { ...current, state: "STOPPING" },
      };
    });
  }

  async function startProfile(profile: LaunchProfile) {
    try {
      const confirmation = await issueLaunchConfirmation(profile.profile_id);
      if (!window.confirm(`确认启动？\n${confirmation.binding_summary}`)) return;
      const info = await startLaunchProfile(
        profile.profile_id,
        confirmation.confirmation_token,
      );
      const resolved = rememberStartedSession(profile, info);
      setNotice({
        tone: "success",
        text:
          resolved.state === "RUNNING" || resolved.state === "STARTING"
            ? `会话已启动 PID ${resolved.pid ?? "?"}`
            : `会话已结束，退出码 ${resolved.exit_code ?? "?"}`,
      });
    } catch (error) {
      showError(error);
    }
  }

  async function stopSession(session: LaunchSessionInfo) {
    try {
      await stopLaunchSession(session.launch_session_id);
      markSessionStopping(session);
      setNotice({ tone: "info", text: `已请求停止 PID ${session.pid ?? "?"}` });
    } catch (error) {
      showError(error);
    }
  }

  async function startAll() {
    if (bulkAction || runtimeSnapshot.startableProfiles.length === 0) return;
    const generation = projectContextGenerationRef.current;
    setBulkAction("start");

    try {
      const confirmations: Array<{
        profile: LaunchProfile;
        confirmation: Awaited<ReturnType<typeof issueLaunchConfirmation>>;
      }> = [];
      for (const profile of runtimeSnapshot.startableProfiles) {
        confirmations.push({
          profile,
          confirmation: await issueLaunchConfirmation(profile.profile_id),
        });
      }

      const summary = confirmations
        .map(
          ({ profile, confirmation }) =>
            `${labelStatus(profile.process_role)}：${confirmation.binding_summary}`,
        )
        .join("\n\n");
      if (
        !window.confirm(
          `确认启动当前项目的 ${confirmations.length} 个配置？\n\n${summary}`,
        )
      ) {
        if (generation === projectContextGenerationRef.current) {
          setNotice({ tone: "info", text: "已取消批量启动" });
        }
        return;
      }

      let succeeded = 0;
      const failures: string[] = [];
      for (const { profile, confirmation } of confirmations) {
        try {
          const info = await startLaunchProfile(
            profile.profile_id,
            confirmation.confirmation_token,
          );
          rememberStartedSession(profile, info);
          succeeded += 1;
        } catch (error) {
          failures.push(
            `${labelStatus(profile.process_role)}：${labelErrorText(String(error))}`,
          );
        }
      }

      if (generation === projectContextGenerationRef.current) {
        setNotice({
          tone: failures.length === 0 ? "success" : "error",
          text:
            failures.length === 0
              ? `已启动 ${succeeded} 个配置`
              : `批量启动完成：成功 ${succeeded}，失败 ${failures.length}。${failures.join("；")}`,
        });
      }
    } catch (error) {
      if (generation === projectContextGenerationRef.current) showError(error);
    } finally {
      setBulkAction(null);
    }
  }

  async function stopAll() {
    if (bulkAction || runtimeSnapshot.stoppableSessions.length === 0) return;
    const generation = projectContextGenerationRef.current;
    const targets = runtimeSnapshot.stoppableSessions;
    if (!window.confirm(`确认停止当前项目的 ${targets.length} 个运行会话？`)) {
      return;
    }

    setBulkAction("stop");
    let succeeded = 0;
    const failures: string[] = [];
    try {
      for (const session of targets) {
        try {
          await stopLaunchSession(session.launch_session_id);
          markSessionStopping(session);
          succeeded += 1;
        } catch (error) {
          failures.push(`PID ${session.pid ?? "?"}：${labelErrorText(String(error))}`);
        }
      }
      if (generation === projectContextGenerationRef.current) {
        setNotice({
          tone: failures.length === 0 ? "success" : "error",
          text:
            failures.length === 0
              ? `已请求停止 ${succeeded} 个会话`
              : `批量停止完成：成功 ${succeeded}，失败 ${failures.length}。${failures.join("；")}`,
        });
      }
    } finally {
      setBulkAction(null);
    }
  }

  function getActiveSession(profileId: string) {
    return activeSessionForProfile(sessionsById, profileId);
  }

  return {
    projects,
    rootPath,
    projectId,
    currentProject,
    candidates,
    profiles,
    selectedCandidate,
    role,
    command,
    workdir,
    sessionsById,
    lastSessionByProfile,
    logsBySessionId,
    loading,
    bulkAction,
    notice,
    view,
    runtimeSnapshot,
    stats,
    setView,
    setRole,
    setCommand,
    setWorkdir,
    pickDirectory,
    editRootPath,
    openProject,
    removeSavedProject,
    scanProject,
    selectCandidate,
    clearComposer,
    saveProfile,
    removeProfile,
    startProfile,
    stopSession,
    startAll,
    stopAll,
    getActiveSession,
  };
}
