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
  const [message, setMessage] = useState<string | null>(null);
  const terminalEventsRef = useRef<Record<string, TerminalEvent>>({});
  const lastSessionByProfileRef = useRef<LastSessionByProfile>({});

  async function refreshProjects() {
    setProjects(await listProjects());
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
      setRootPath(picked);
      setProjectId("");
      setProfiles([]);
      setCandidates([]);
      setSelected(null);
      setMessage("目录已选择，扫描后会保存到项目列表");
    }
  }

  async function openProject(project: ProjectInfo) {
    setRootPath(project.root_path);
    setProjectId(project.project_id);
    setCandidates([]);
    setSelected(null);
    setProfiles(await listLaunchProfiles(project.project_id));
    setMessage(`已打开项目：${project.name}`);
  }

  async function onRemoveProject(project: ProjectInfo) {
    if (
      !window.confirm(
        `移除项目记录及其启动配置，不删除磁盘文件。\n运行中的项目需要先停止。\n\n${project.name}`,
      )
    ) {
      return;
    }
    try {
      await removeProject(project.project_id);
      if (projectId === project.project_id) {
        setRootPath("");
        setProjectId("");
        setCandidates([]);
        setProfiles([]);
        setSelected(null);
      }
      await refreshProjects();
      setMessage("项目记录与启动配置已移除，磁盘文件未删除");
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  async function onScan() {
    if (!rootPath) return;
    setLoading(true);
    setMessage(null);
    try {
      const result = await scanProjectDirectory(rootPath);
      const project = await upsertProject(result.root_path);
      setRootPath(project.root_path);
      setProjectId(project.project_id);
      setCandidates(result.candidates);
      setProfiles(await listLaunchProfiles(project.project_id));
      await refreshProjects();
      setMessage(`项目已保存：${project.name}`);
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    } finally {
      setLoading(false);
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
    try {
      await saveLaunchProfile({
        projectId,
        processRole: role,
        workingDirectory: workdir,
        command,
        sourceCandidateId: selected?.id,
      });
      setProfiles(await listLaunchProfiles(projectId));
      setMessage("启动配置已保存");
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  async function onRemoveProfile(profile: LaunchProfile) {
    if (!window.confirm(`移除这条启动配置？\n\n${profile.command}`)) return;
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

      setProfiles(await listLaunchProfiles(profile.project_id));
      setMessage("启动配置已移除");
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
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
      const pendingTerminal = terminalEventsRef.current[info.launch_session_id];
      const resolvedInfo = pendingTerminal
        ? {
            ...info,
            state: terminalState(pendingTerminal, info.state),
            exit_code: pendingTerminal.exitCode ?? info.exit_code,
          }
        : info;
      const previousSessionId =
        lastSessionByProfileRef.current[profile.profile_id];
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
        if (
          previousSessionId &&
          previousSessionId !== info.launch_session_id
        ) {
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
        if (
          previousSessionId &&
          previousSessionId !== info.launch_session_id
        ) {
          delete next[previousSessionId];
        }
        return next;
      });
      if (pendingTerminal) {
        delete terminalEventsRef.current[info.launch_session_id];
      }
      setMessage(
        pendingTerminal
          ? `会话已结束，退出码 ${pendingTerminal.exitCode ?? "?"}`
          : `会话已启动 PID ${info.pid ?? "?"}`,
      );
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  async function onStop(session: LaunchSessionInfo) {
    try {
      await stopLaunchSession(session.launch_session_id);
      setSessionsById((prev) => {
        const current = prev[session.launch_session_id];
        if (!current || !ACTIVE_SESSION_STATES.has(current.state)) return prev;
        return {
          ...prev,
          [session.launch_session_id]: { ...current, state: "STOPPING" },
        };
      });
      setMessage(`已请求停止 PID ${session.pid ?? "?"}`);
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  return (
    <>
      <PageHeader
        title="项目管理"
        description="保存项目、扫描技术栈、配置并启动前后端会话"
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
                        disabled={activeSession.state === "STOPPING"}
                      >
                        {activeSession.state === "STOPPING" ? "停止中…" : "停止"}
                      </button>
                    ) : (
                      <>
                        <button type="button" onClick={() => onStart(profile)}>
                          启动
                        </button>
                        <button
                          type="button"
                          className="secondary"
                          onClick={() => void onRemoveProfile(profile)}
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
