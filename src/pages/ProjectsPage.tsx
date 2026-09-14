import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { PageHeader } from "../components/PageHeader";
import {
  issueLaunchConfirmation,
  listLaunchProfiles,
  listLaunchSessions,
  projectIdForPath,
  saveLaunchProfile,
  scanProjectDirectory,
  startLaunchProfile,
  stopLaunchSession,
} from "../ipc/client";
import type {
  LaunchProfile,
  LaunchSessionInfo,
  TechnologyCandidate,
} from "../ipc/types";
import { formatDisplayPath } from "../lib/formatDisplay";
import { labelErrorText, labelStatus } from "../lib/statusLabels";

const ACTIVE_SESSION_STATES = new Set(["STARTING", "RUNNING", "STOPPING"]);

type SessionsById = Record<string, LaunchSessionInfo>;
type LogsBySessionId = Record<string, string[]>;

function indexSessions(sessions: LaunchSessionInfo[]): SessionsById {
  return Object.fromEntries(
    sessions.map((session) => [session.launch_session_id, session]),
  );
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

export function ProjectsPage() {
  const [rootPath, setRootPath] = useState("");
  const [projectId, setProjectId] = useState("");
  const [candidates, setCandidates] = useState<TechnologyCandidate[]>([]);
  const [profiles, setProfiles] = useState<LaunchProfile[]>([]);
  const [selected, setSelected] = useState<TechnologyCandidate | null>(null);
  const [role, setRole] = useState("frontend");
  const [command, setCommand] = useState("");
  const [workdir, setWorkdir] = useState("");
  const [sessionsById, setSessionsById] = useState<SessionsById>({});
  const [logsBySessionId, setLogsBySessionId] = useState<LogsBySessionId>({});
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;

    void listLaunchSessions()
      .then((sessions) => {
        if (!disposed) setSessionsById(indexSessions(sessions));
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
    }>("launch_output", (event) => {
      const p = event.payload;
      const line = p.final
        ? `[${labelStatus("exit")}] 退出码=${p.exitCode ?? "?"}`
        : `[${labelStatus(p.stream)}] ${p.chunk}`;

      setLogsBySessionId((prev) => ({
        ...prev,
        [p.launchSessionId]: [...(prev[p.launchSessionId] ?? []), line],
      }));

      if (!p.final) return;

      // finalState is currently hard-coded by the backend. Mark terminal locally first,
      // then refresh from the authoritative runtime session list.
      setSessionsById((prev) => {
        const current = prev[p.launchSessionId];
        if (!current) return prev;
        return {
          ...prev,
          [p.launchSessionId]: {
            ...current,
            state: "STOPPED",
            exit_code: p.exitCode ?? current.exit_code,
          },
        };
      });

      void listLaunchSessions()
        .then((sessions) => {
          if (!disposed) setSessionsById(indexSessions(sessions));
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
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setRootPath(selected);
      const id = await projectIdForPath(selected);
      setProjectId(id);
    }
  }

  async function onScan() {
    if (!rootPath) return;
    setLoading(true);
    setMessage(null);
    try {
      const result = await scanProjectDirectory(rootPath);
      setCandidates(result.candidates);
      const id = await projectIdForPath(result.root_path);
      setProjectId(id);
      setProfiles(await listLaunchProfiles(id));
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    } finally {
      setLoading(false);
    }
  }

  function selectCandidate(c: TechnologyCandidate) {
    setSelected(c);
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

  async function onStart(profile: LaunchProfile) {
    try {
      const confirm = await issueLaunchConfirmation(profile.profile_id);
      const ok = window.confirm(`确认启动？\n${confirm.binding_summary}`);
      if (!ok) return;

      const info = await startLaunchProfile(
        profile.profile_id,
        confirm.confirmation_token,
      );
      setSessionsById((prev) => ({
        ...prev,
        [info.launch_session_id]: info,
      }));
      setLogsBySessionId((prev) => ({
        ...prev,
        [info.launch_session_id]: [],
      }));
      setMessage(`会话已启动 PID ${info.pid ?? "?"}`);
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
        description="扫描技术栈、配置并启动前后端会话"
      />
      <div className="card toolbar-card env-toolbar">
        <input
          className="env-path-input"
          value={rootPath}
          onChange={(e) => setRootPath(e.target.value)}
          placeholder="项目根目录"
        />
        <button type="button" className="secondary" onClick={pickDir}>
          选择项目目录
        </button>
        <button type="button" onClick={onScan} disabled={loading || !rootPath}>
          {loading ? "扫描中…" : "扫描项目"}
        </button>
      </div>
      {message && <div className="feedback-banner">{message}</div>}
      {candidates.length === 0 && !loading && (
        <div className="card">
          <div className="empty">选择目录后扫描技术栈证据。</div>
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
              const sessionLogs = activeSession
                ? logsBySessionId[activeSession.launch_session_id] ?? []
                : [];

              return (
                <div key={profile.profile_id} className="env-candidate">
                  <div className="env-candidate-head">
                    <span className="env-badge muted">
                      {labelStatus(profile.process_role)}
                    </span>
                    {activeSession && (
                      <span className="env-badge ok">
                        {labelStatus(activeSession.state)} · PID {activeSession.pid ?? "—"}
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
                      <button type="button" onClick={() => onStart(profile)}>
                        启动
                      </button>
                    )}
                  </div>
                  {activeSession && (
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
