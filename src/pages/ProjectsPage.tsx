import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { PageHeader } from "../components/PageHeader";
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
} from "../ipc/client";
import type {
  LaunchProfile,
  LaunchSessionInfo,
  ProjectInfo,
  TechnologyCandidate,
} from "../ipc/types";
import { formatDisplayPath } from "../lib/formatDisplay";
import { buildProjectRuntimeSnapshot } from "../lib/projectRuntime";
import { labelErrorText, labelStatus } from "../lib/statusLabels";

const ACTIVE_SESSION_STATES = new Set(["STARTING", "RUNNING", "STOPPING"]);
const MAX_PENDING_TERMINAL_EVENTS = 64;
const MAX_LOG_LINES_PER_SESSION = 2000;

type SessionsById = Record<string, LaunchSessionInfo>;
type LogsBySessionId = Record<string, string[]>;
type LastSessionByProfile = Record<string, string>;
type TerminalEvent = {
  exitCode?: number;
  finalState?: string;
};

function indexSessions(sessions: LaunchSessionInfo[]): SessionsById {
  return Object.fromEntries(
    sessions.map((session) => [session.launch_session_id, session]),
  );
}

function indexLastSessions(sessions: LaunchSessionInfo[]): LastSessionByProfile {
  return Object.fromEntries(
    sessions.map((session) => [session.profile_id, session.launch_session_id]),
  );
}

function rememberPendingTerminalEvent(
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

function appendSessionLog(existing: string[], line: string): string[] {
  if (existing.length < MAX_LOG_LINES_PER_SESSION) {
    return [...existing, line];
  }
  return [
    ...existing.slice(existing.length - MAX_LOG_LINES_PER_SESSION + 1),
    line,
  ];
}

function activeSessionForProfile(
  sessionsById: SessionsById,
  profileId: string,
): LaunchSessionInfo | undefined {
  return Object.values(sessionsById).find(
    (session) =>
      session.profile_id === profileId && ACTIVE_SESSION_STATES.has(session.state),
  );
}

function suggestedRole(candidate: TechnologyCandidate): "frontend" | "backend" {
  return candidate.stack.toUpperCase() === "NODE" ? "frontend" : "backend";
}

function terminalState(
  event: TerminalEvent,
  currentState?: string,
): string {
  if (event.finalState) return event.finalState;
  if (currentState === "STOPPING") return "STOPPED";
  if (event.exitCode !== undefined && event.exitCode !== 0) return "FAILED";
  return "STOPPED";
}

function sessionBadgeClass(session: LaunchSessionInfo): string {
  if (ACTIVE_SESSION_STATES.has(session.state)) return "env-badge ok";
  if (session.state === "FAILED" || (session.exit_code ?? 0) !== 0) {
    return "env-badge err";
  }
  return "env-badge muted";
}

export function ProjectsPage() {
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [rootPath, setRootPath] = useState("");
  const [projectId, setProjectId] = useState("");
  const [candidates, setCandidates] = useState<TechnologyCandidate[]>([]);
  const [profiles, setProfiles] = useState<LaunchProfile[]>([]);
  const [selected, setSelected] = useState<TechnologyCandidate | null>(null);
  const [role, setRole] = useState("frontend");
  const [command, setCommand] = useState("");
  const [workdir, setWorkdir] = useState("");
  const [sessionsById, setSessionsById] = useState<SessionsById>({});
  const [lastSessionByProfile, setLastSessionByProfile] =
    useState<LastSessionByProfile>({});
  const [logsBySessionId, setLogsBySessionId] = useState<LogsBySessionId>({});
  const [loading, setLoading] = useState(false);
  const [bulkAction, setBulkAction] = useState<"start" | "stop" | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const terminalEventsRef = useRef<Record<string, TerminalEvent>>({});
  const lastSessionByProfileRef = useRef<LastSessionByProfile>({});
  const projectContextGenerationRef = useRef(0);

  function invalidateProjectContext() {
    projectContextGenerationRef.current += 1;
    setLoading(false);
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
      .catch((e) => {
        if (!disposed) setMessage(labelErrorText(String(e)));
      });

    const unlisten = listen<{
      launchSessionId: string;
      stream: string;
      chunk: string;
      final?: boolean;
      exitCode?: number;
      finalState?: string;
    }>("launch_output", (event) => {
      const p = event.payload;
      const line = p.final
        ? `[${labelStatus("exit")}] 退出码=${p.exitCode ?? "?"}`
        : `[${labelStatus(p.stream)}] ${p.chunk}`;

      setLogsBySessionId((prev) => ({
        ...prev,
        [p.launchSessionId]: appendSessionLog(
          prev[p.launchSessionId] ?? [],
          line,
        ),
      }));

      if (!p.final) return;

      const terminalEvent: TerminalEvent = {
        exitCode: p.exitCode,
        finalState: p.finalState,
      };
      rememberPendingTerminalEvent(
        terminalEventsRef.current,
        p.launchSessionId,
        terminalEvent,
      );

      setSessionsById((prev) => {
        const current = prev[p.launchSessionId];
        if (!current) return prev;
        return {
          ...prev,
          [p.launchSessionId]: {
            ...current,
            state: terminalState(terminalEvent, current.state),
            exit_code: p.exitCode ?? current.exit_code,
          },
        };
      });

      void listLaunchSessions()
        .then((sessions) => {
          if (disposed) return;
          const activeLastSessions = indexLastSessions(sessions);
          setSessionsById((prev) => ({ ...prev, ...indexSessions(sessions) }));
          if (Object.keys(activeLastSessions).length > 0) {
            const nextLastSessions = {
              ...lastSessionByProfileRef.current,
              ...activeLastSessions,
            };
            lastSessionByProfileRef.current = nextLastSessions;
            setLastSessionByProfile(nextLastSessions);
          }
        })
        .catch(() => {
          // Keep the event-derived terminal state if the refresh fails.
        });
    });

    return () => {
      disposed = true;
      void unlisten.then((fn) => fn());
    };
  }, []);

  async function pickDir() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") {
      invalidateProjectContext();
      setRootPath(picked);
      setProjectId("");
      setProfiles([]);
      setCandidates([]);
      setSelected(null);
      setMessage("目录已选择，扫描后会保存到项目列表");
    }
  }

  async function openProject(project: ProjectInfo) {
    const generation = projectContextGenerationRef.current + 1;
    projectContextGenerationRef.current = generation;
    setLoading(false);
    setRootPath(project.root_path);
    setProjectId(project.project_id);
    setCandidates([]);
    setSelected(null);
    setProfiles([]);
    try {
      const nextProfiles = await listLaunchProfiles(project.project_id);
      if (generation !== projectContextGenerationRef.current) return;
      setProfiles(nextProfiles);
      setMessage(`已打开项目：${project.name}`);
    } catch (e) {
      if (generation === projectContextGenerationRef.current) {
        setMessage(labelErrorText(String(e)));
      }
    }
  }

  async function onRemoveProject(project: ProjectInfo) {
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
        invalidateProjectContext();
        setRootPath("");
        setProjectId("");
        setCandidates([]);
        setProfiles([]);
        setSelected(null);
      }
      setMessage("项目记录与启动配置已移除，磁盘文件未删除");
    } catch (e) {
      if (generation === projectContextGenerationRef.current) {
        setMessage(labelErrorText(String(e)));
      }
    }
  }

  async function onScan() {
    if (!rootPath) return;
    const generation = projectContextGenerationRef.current + 1;
    projectContextGenerationRef.current = generation;
    const requestedRoot = rootPath;
    setLoading(true);
    setMessage(null);
    try {
      const result = await scanProjectDirectory(requestedRoot);
      if (generation !== projectContextGenerationRef.current) return;
      const project = await upsertProject(result.root_path);
      if (generation !== projectContextGenerationRef.current) return;
      const nextProfiles = await listLaunchProfiles(project.project_id);
      if (generation !== projectContextGenerationRef.current) return;
      const savedProjects = await listProjects();
      if (generation !== projectContextGenerationRef.current) return;

      setRootPath(project.root_path);
      setProjectId(project.project_id);
      setCandidates(result.candidates);
      setProfiles(nextProfiles);
      setProjects(savedProjects);
      setMessage(`项目已保存：${project.name}`);
    } catch (e) {
      if (generation === projectContextGenerationRef.current) {
        setMessage(labelErrorText(String(e)));
      }
    } finally {
      if (generation === projectContextGenerationRef.current) {
        setLoading(false);
      }
    }
  }

  function selectCandidate(c: TechnologyCandidate) {
    setSelected(c);
    setRole(suggestedRole(c));
    setWorkdir(c.directory);
    setCommand(c.suggested_command ?? "");
  }

  async function onSaveProfile() {
    if (!projectId || !workdir || !command) return;
    const generation = projectContextGenerationRef.current;
    const targetProjectId = projectId;
    try {
      await saveLaunchProfile({
        projectId: targetProjectId,
        processRole: role,
        workingDirectory: workdir,
        command,
        sourceCandidateId: selected?.id,
      });
      const nextProfiles = await listLaunchProfiles(targetProjectId);
      if (generation !== projectContextGenerationRef.current) return;
      setProfiles(nextProfiles);
      setMessage("启动配置已保存");
    } catch (e) {
      if (generation === projectContextGenerationRef.current) {
        setMessage(labelErrorText(String(e)));
      }
    }
  }

  async function onRemoveProfile(profile: LaunchProfile) {
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
        setSessionsById((prev) => {
          const next = { ...prev };
          delete next[lastSessionId];
          return next;
        });
        setLogsBySessionId((prev) => {
          const next = { ...prev };
          delete next[lastSessionId];
          return next;
        });
        delete terminalEventsRef.current[lastSessionId];
      }

      const nextProfiles = await listLaunchProfiles(profile.project_id);
      if (generation !== projectContextGenerationRef.current) return;
      setProfiles(nextProfiles);
      setMessage("启动配置已移除");
    } catch (e) {
      if (generation === projectContextGenerationRef.current) {
        setMessage(labelErrorText(String(e)));
      }
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

    setSessionsById((prev) => {
      const next = {
        ...prev,
        [info.launch_session_id]: resolvedInfo,
      };
      if (previousSessionId && previousSessionId !== info.launch_session_id) {
        delete next[previousSessionId];
      }
      return next;
    });
    setLastSessionByProfile(nextLastSessions);
    setLogsBySessionId((prev) => {
      const next = {
        ...prev,
        [info.launch_session_id]: prev[info.launch_session_id] ?? [],
      };
      if (previousSessionId && previousSessionId !== info.launch_session_id) {
        delete next[previousSessionId];
      }
      return next;
    });
    if (pendingTerminal) {
      delete terminalEventsRef.current[info.launch_session_id];
    }
    return resolvedInfo;
  }

  function markSessionStopping(session: LaunchSessionInfo) {
    setSessionsById((prev) => {
      const current = prev[session.launch_session_id];
      if (!current || !ACTIVE_SESSION_STATES.has(current.state)) return prev;
      return {
        ...prev,
        [session.launch_session_id]: { ...current, state: "STOPPING" },
      };
    });
  }

  async function onStart(profile: LaunchProfile) {
    try {
      const confirm = await issueLaunchConfirmation(profile.profile_id);
      const ok = window.confirm(`确认启动？\n${confirm.binding_summary}`);
      if (!ok) return;

      const info = await startLaunchProfile(
        profile.profile_id,
        confirm.confirmation_token,
      );
      const resolvedInfo = rememberStartedSession(profile, info);
      setMessage(
        resolvedInfo.state === "RUNNING" || resolvedInfo.state === "STARTING"
          ? `会话已启动 PID ${resolvedInfo.pid ?? "?"}`
          : `会话已结束，退出码 ${resolvedInfo.exit_code ?? "?"}`,
      );
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  async function onStop(session: LaunchSessionInfo) {
    try {
      await stopLaunchSession(session.launch_session_id);
      markSessionStopping(session);
      setMessage(`已请求停止 PID ${session.pid ?? "?"}`);
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  const runtimeSnapshot = buildProjectRuntimeSnapshot(profiles, sessionsById);

  async function onStartAll() {
    if (bulkAction || runtimeSnapshot.startableProfiles.length === 0) return;
    const generation = projectContextGenerationRef.current;
    setBulkAction("start");
    try {
      const confirmations: Array<{
        profile: LaunchProfile;
        confirmation: Awaited<ReturnType<typeof issueLaunchConfirmation>>;
      }> = [];
      for (const profile of runtimeSnapshot.startableProfiles) {
        const confirmation = await issueLaunchConfirmation(profile.profile_id);
        confirmations.push({ profile, confirmation });
      }

      const summary = confirmations
        .map(
          ({ profile, confirmation }) =>
            `${labelStatus(profile.process_role)}：${confirmation.binding_summary}`,
        )
        .join("\n\n");
      const ok = window.confirm(
        `确认启动当前项目的 ${confirmations.length} 个配置？\n\n${summary}`,
      );
      if (!ok) {
        if (generation === projectContextGenerationRef.current) {
          setMessage("已取消批量启动");
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
        } catch (e) {
          failures.push(
            `${labelStatus(profile.process_role)}：${labelErrorText(String(e))}`,
          );
        }
      }

      if (generation === projectContextGenerationRef.current) {
        setMessage(
          failures.length === 0
            ? `已启动 ${succeeded} 个配置`
            : `批量启动完成：成功 ${succeeded}，失败 ${failures.length}。${failures.join("；")}`,
        );
      }
    } catch (e) {
      if (generation === projectContextGenerationRef.current) {
        setMessage(labelErrorText(String(e)));
      }
    } finally {
      setBulkAction(null);
    }
  }

  async function onStopAll() {
    if (bulkAction || runtimeSnapshot.stoppableSessions.length === 0) return;
    const generation = projectContextGenerationRef.current;
    const targets = runtimeSnapshot.stoppableSessions;
    const ok = window.confirm(`确认停止当前项目的 ${targets.length} 个运行会话？`);
    if (!ok) return;

    setBulkAction("stop");
    let succeeded = 0;
    const failures: string[] = [];
    try {
      for (const session of targets) {
        try {
          await stopLaunchSession(session.launch_session_id);
          markSessionStopping(session);
          succeeded += 1;
        } catch (e) {
          failures.push(`PID ${session.pid ?? "?"}：${labelErrorText(String(e))}`);
        }
      }
      if (generation === projectContextGenerationRef.current) {
        setMessage(
          failures.length === 0
            ? `已请求停止 ${succeeded} 个会话`
            : `批量停止完成：成功 ${succeeded}，失败 ${failures.length}。${failures.join("；")}`,
        );
      }
    } finally {
      setBulkAction(null);
    }
  }

  return (
    <>
      <PageHeader
        title="项目管理"
        description="保存项目、扫描技术栈、配置并统一启停前后端会话"
      />

      {projects.length > 0 && (
        <div className="card">
          <div className="card-header">
            <h3>我的项目</h3>
            <span className="card-meta">{projects.length} 个</span>
          </div>
          <div className="card-body env-candidate-list">
            {projects.map((project) => (
              <div key={project.project_id} className="env-candidate">
                <div className="env-candidate-head">
                  <span className="env-tag">{project.name}</span>
                  {projectId === project.project_id && (
                    <span className="env-badge ok">当前项目</span>
                  )}
                </div>
                <div className="env-path" title={project.root_path}>
                  {formatDisplayPath(project.root_path)}
                </div>
                <div className="list-card-actions">
                  <button type="button" onClick={() => void openProject(project)}>
                    打开
                  </button>
                  <button
                    type="button"
                    className="secondary"
                    onClick={() => void onRemoveProject(project)}
                  >
                    移除记录
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="card toolbar-card env-toolbar">
        <input
          className="env-path-input"
          value={rootPath}
          onChange={(e) => {
            invalidateProjectContext();
            setRootPath(e.target.value);
            setProjectId("");
            setProfiles([]);
            setCandidates([]);
            setSelected(null);
          }}
          placeholder="项目根目录"
        />
        <button type="button" className="secondary" onClick={pickDir}>
          选择项目目录
        </button>
        <button type="button" onClick={onScan} disabled={loading || !rootPath}>
          {loading ? "扫描中…" : projectId ? "重新扫描" : "扫描并添加"}
        </button>
      </div>

      {message && <div className="feedback-banner">{message}</div>}

      {candidates.length === 0 && !loading && rootPath && (
        <div className="card">
          <div className="empty">扫描项目以识别技术栈和启动配置。</div>
        </div>
      )}

      {candidates.length > 0 && (
        <div className="env-candidate-list">
          {candidates.map((c) => (
            <div key={c.id} className="env-candidate">
              <div className="env-candidate-head">
                <span className="env-tag">{labelStatus(c.stack)}</span>
                <span
                  className={
                    c.status === "CONFLICT" ? "env-badge err" : "env-badge ok"
                  }
                >
                  {labelStatus(c.status)}
                </span>
              </div>
              <div className="env-path" title={c.directory}>
                {formatDisplayPath(c.directory)}
              </div>
              <div className="env-meta">
                <span className="env-meta-label">证据</span>
                <span className="env-version">{c.evidence_file}</span>
              </div>
              {c.suggested_command && (
                <div className="env-meta">
                  <span className="env-meta-label">建议</span>
                  <span className="env-version">{c.suggested_command}</span>
                </div>
              )}
              <div className="list-card-actions">
                <button type="button" onClick={() => selectCandidate(c)}>
                  配置启动
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {selected && (
        <div className="card">
          <div className="card-header">
            <h3>启动配置</h3>
          </div>
          <div className="card-body">
            <div className="form-row">
              <select value={role} onChange={(e) => setRole(e.target.value)}>
                <option value="frontend">前端</option>
                <option value="backend">后端</option>
              </select>
              <input
                className="input-grow"
                value={workdir}
                onChange={(e) => setWorkdir(e.target.value)}
                placeholder="工作目录"
              />
            </div>
            <div className="form-row">
              <input
                className="input-grow"
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder="启动命令"
              />
              <button type="button" onClick={onSaveProfile}>
                保存启动配置
              </button>
            </div>
          </div>
        </div>
      )}

      {profiles.length > 0 && (
        <div className="card">
          <div className="card-header">
            <h3>已保存配置</h3>
            <div className="project-runtime-actions">
              <span
                className={
                  runtimeSnapshot.activeSessions.length > 0
                    ? "env-badge ok"
                    : "env-badge muted"
                }
              >
                运行 {runtimeSnapshot.activeSessions.length}/{profiles.length}
              </span>
              <button
                type="button"
                className="btn-sm"
                onClick={() => void onStartAll()}
                disabled={
                  bulkAction !== null || runtimeSnapshot.startableProfiles.length === 0
                }
              >
                {bulkAction === "start"
                  ? "启动中…"
                  : `全部启动 (${runtimeSnapshot.startableProfiles.length})`}
              </button>
              <button
                type="button"
                className="secondary btn-sm"
                onClick={() => void onStopAll()}
                disabled={
                  bulkAction !== null || runtimeSnapshot.stoppableSessions.length === 0
                }
              >
                {bulkAction === "stop"
                  ? "停止中…"
                  : `全部停止 (${runtimeSnapshot.stoppableSessions.length})`}
              </button>
            </div>
          </div>
          <div className="card-body env-candidate-list">
            {profiles.map((profile) => {
              const activeSession = activeSessionForProfile(
                sessionsById,
                profile.profile_id,
              );
              const lastSessionId = lastSessionByProfile[profile.profile_id];
              const displaySession =
                activeSession ?? (lastSessionId ? sessionsById[lastSessionId] : undefined);
              const sessionLogs = displaySession
                ? logsBySessionId[displaySession.launch_session_id] ?? []
                : [];

              return (
                <div key={profile.profile_id} className="env-candidate">
                  <div className="env-candidate-head">
                    <span className="env-badge muted">
                      {labelStatus(profile.process_role)}
                    </span>
                    {displaySession && (
                      <span className={sessionBadgeClass(displaySession)}>
                        {labelStatus(displaySession.state)} · PID {displaySession.pid ?? "—"}
                        {displaySession.exit_code !== undefined &&
                        displaySession.exit_code !== null
                          ? ` · 退出码 ${displaySession.exit_code}`
                          : ""}
                      </span>
                    )}
                  </div>
                  <div className="env-path">{profile.command}</div>
                  <div className="env-meta">
                    <span className="env-meta-label">目录</span>
                    <span className="env-version">
                      {formatDisplayPath(profile.working_directory)}
                    </span>
                  </div>
                  <div className="list-card-actions">
                    {activeSession ? (
                      <button
                        type="button"
                        className="secondary"
                        onClick={() => onStop(activeSession)}
                        disabled={
                          bulkAction !== null || activeSession.state === "STOPPING"
                        }
                      >
                        {activeSession.state === "STOPPING" ? "停止中…" : "停止"}
                      </button>
                    ) : (
                      <>
                        <button
                          type="button"
                          onClick={() => onStart(profile)}
                          disabled={bulkAction !== null}
                        >
                          启动
                        </button>
                        <button
                          type="button"
                          className="secondary"
                          onClick={() => void onRemoveProfile(profile)}
                          disabled={bulkAction !== null}
                        >
                          移除配置
                        </button>
                      </>
                    )}
                  </div>
                  {displaySession && (
                    <div className="log-panel">
                      {sessionLogs.join("\n") || "暂无输出"}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </>
  );
}
