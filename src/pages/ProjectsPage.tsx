import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import {
  Activity,
  Boxes,
  FolderKanban,
  FolderOpen,
  Play,
  RefreshCw,
  ScanSearch,
  Settings2,
  Square,
  Terminal,
  Trash2,
} from "lucide-react";
import { PageHeader } from "../components/PageHeader";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../components/ui/card";
import { Input } from "../components/ui/input";
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
  if (ACTIVE_SESSION_STATES.has(session.state)) {
    return "border-emerald-500/25 bg-emerald-500/10 text-emerald-500";
  }
  if (session.state === "FAILED" || (session.exit_code ?? 0) !== 0) {
    return "border-destructive/25 bg-destructive/10 text-destructive";
  }
  return "border-border bg-muted text-muted-foreground";
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
        description="项目发现、启动配置与本地运行控制"
      />

      <div className="space-y-4 pb-2">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-4">
            <div className="flex min-w-0 items-center gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                <FolderKanban className="size-4" />
              </div>
              <div className="min-w-0">
                <CardTitle>我的项目</CardTitle>
                <CardDescription>打开已有项目，或从本机目录添加新的项目。</CardDescription>
              </div>
            </div>
            <Badge variant="secondary">{projects.length} 个</Badge>
          </CardHeader>
          <CardContent>
            {projects.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border px-5 py-8 text-center text-sm text-muted-foreground">
                还没有项目。选择一个本机目录并扫描后，会自动加入项目库。
              </div>
            ) : (
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                {projects.map((project) => {
                  const isCurrent = projectId === project.project_id;
                  return (
                    <Card
                      key={project.project_id}
                      className={
                        isCurrent
                          ? "gap-3 border-primary/60 bg-primary/[0.04] shadow-md ring-1 ring-primary/15"
                          : "gap-3 bg-background/40 shadow-none transition-colors hover:border-primary/35"
                      }
                    >
                      <CardHeader className="gap-2 px-4 pt-4">
                        <div className="flex items-start justify-between gap-3">
                          <CardTitle className="min-w-0 truncate text-sm">
                            {project.name}
                          </CardTitle>
                          {isCurrent && (
                            <Badge className="shrink-0">当前</Badge>
                          )}
                        </div>
                        <CardDescription
                          className="truncate font-mono text-xs"
                          title={project.root_path}
                        >
                          {formatDisplayPath(project.root_path)}
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="mt-auto flex gap-2 px-4 pb-4">
                        <Button
                          size="sm"
                          className="flex-1"
                          onClick={() => void openProject(project)}
                        >
                          <FolderOpen />
                          打开
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="text-muted-foreground hover:text-destructive"
                          onClick={() => void onRemoveProject(project)}
                          aria-label={`移除项目 ${project.name}`}
                        >
                          <Trash2 />
                        </Button>
                      </CardContent>
                    </Card>
                  );
                })}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardContent className="pt-5">
            <div className="grid items-center gap-3 lg:grid-cols-[minmax(0,1fr)_auto_auto]">
              <div className="relative min-w-0">
                <FolderOpen className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  className="pl-9 font-mono text-xs"
                  value={rootPath}
                  onChange={(e) => {
                    invalidateProjectContext();
                    setRootPath(e.target.value);
                    setProjectId("");
                    setProfiles([]);
                    setCandidates([]);
                    setSelected(null);
                  }}
                  placeholder="选择或输入项目根目录"
                />
              </div>
              <Button variant="outline" onClick={pickDir}>
                <FolderOpen />
                选择目录
              </Button>
              <Button onClick={onScan} disabled={loading || !rootPath}>
                {loading ? <RefreshCw className="animate-spin" /> : <ScanSearch />}
                {loading ? "扫描中…" : projectId ? "重新扫描" : "扫描并添加"}
              </Button>
            </div>
          </CardContent>
        </Card>

        {message && (
          <div className="rounded-lg border border-border bg-muted/50 px-4 py-3 text-sm text-muted-foreground">
            {message}
          </div>
        )}

        {candidates.length === 0 && !loading && rootPath && (
          <Card>
            <CardContent className="py-8 text-center text-sm text-muted-foreground">
              扫描项目后，这里会显示识别到的技术栈和建议启动命令。
            </CardContent>
          </Card>
        )}

        {candidates.length > 0 && (
          <Card>
            <CardHeader>
              <div className="flex items-center gap-3">
                <div className="flex size-9 items-center justify-center rounded-lg bg-secondary text-secondary-foreground">
                  <Boxes className="size-4" />
                </div>
                <div>
                  <CardTitle>技术栈扫描</CardTitle>
                  <CardDescription>
                    选择识别结果生成启动配置；扫描结果不会自动执行命令。
                  </CardDescription>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                {candidates.map((candidate) => (
                  <Card key={candidate.id} className="gap-3 bg-background/40 shadow-none">
                    <CardHeader className="gap-2 px-4 pt-4">
                      <div className="flex items-center justify-between gap-2">
                        <Badge variant="secondary">
                          {labelStatus(candidate.stack)}
                        </Badge>
                        <Badge
                          variant="outline"
                          className={
                            candidate.status === "CONFLICT"
                              ? "border-destructive/25 bg-destructive/10 text-destructive"
                              : "border-emerald-500/25 bg-emerald-500/10 text-emerald-500"
                          }
                        >
                          {labelStatus(candidate.status)}
                        </Badge>
                      </div>
                      <CardDescription
                        className="truncate font-mono text-xs"
                        title={candidate.directory}
                      >
                        {formatDisplayPath(candidate.directory)}
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="mt-auto space-y-3 px-4 pb-4">
                      <div className="space-y-1 text-xs text-muted-foreground">
                        <div className="truncate" title={candidate.evidence_file}>
                          证据：{candidate.evidence_file}
                        </div>
                        {candidate.suggested_command && (
                          <div className="rounded-md border border-border bg-muted/40 px-2.5 py-2 font-mono text-foreground">
                            {candidate.suggested_command}
                          </div>
                        )}
                      </div>
                      <Button
                        size="sm"
                        variant={selected?.id === candidate.id ? "secondary" : "outline"}
                        className="w-full"
                        onClick={() => selectCandidate(candidate)}
                      >
                        <Settings2 />
                        {selected?.id === candidate.id ? "正在配置" : "配置启动"}
                      </Button>
                    </CardContent>
                  </Card>
                ))}
              </div>
            </CardContent>
          </Card>
        )}

        {selected && (
          <Card className="border-primary/30">
            <CardHeader>
              <div className="flex items-center gap-3">
                <div className="flex size-9 items-center justify-center rounded-lg bg-primary/10 text-primary">
                  <Settings2 className="size-4" />
                </div>
                <div>
                  <CardTitle>启动配置</CardTitle>
                  <CardDescription>
                    调整进程角色、工作目录和启动命令后保存。
                  </CardDescription>
                </div>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="grid gap-3 md:grid-cols-[140px_minmax(0,1fr)]">
                <select
                  className="h-9 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none focus:border-ring focus:ring-2 focus:ring-ring/40"
                  value={role}
                  onChange={(e) => setRole(e.target.value)}
                >
                  <option value="frontend">前端</option>
                  <option value="backend">后端</option>
                </select>
                <Input
                  className="font-mono text-xs"
                  value={workdir}
                  onChange={(e) => setWorkdir(e.target.value)}
                  placeholder="工作目录"
                />
              </div>
              <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
                <Input
                  className="font-mono text-xs"
                  value={command}
                  onChange={(e) => setCommand(e.target.value)}
                  placeholder="启动命令"
                />
                <Button onClick={onSaveProfile}>
                  <Settings2 />
                  保存启动配置
                </Button>
              </div>
            </CardContent>
          </Card>
        )}

        {profiles.length > 0 && (
          <Card>
            <CardHeader className="sticky top-0 z-10 border-b border-border bg-card/95 pb-4 backdrop-blur supports-[backdrop-filter]:bg-card/85">
              <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
                <div className="flex min-w-0 items-center gap-3">
                  <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                    <Activity className="size-4" />
                  </div>
                  <div className="min-w-0">
                    <CardTitle>运行控制</CardTitle>
                    <CardDescription>
                      {profiles.length} 个启动配置，当前运行 {runtimeSnapshot.activeSessions.length} 个。
                    </CardDescription>
                  </div>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <Badge
                    variant="outline"
                    className={
                      runtimeSnapshot.activeSessions.length > 0
                        ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-500"
                        : "text-muted-foreground"
                    }
                  >
                    运行 {runtimeSnapshot.activeSessions.length}/{profiles.length}
                  </Badge>
                  <Button
                    size="sm"
                    onClick={() => void onStartAll()}
                    disabled={
                      bulkAction !== null || runtimeSnapshot.startableProfiles.length === 0
                    }
                  >
                    {bulkAction === "start" ? (
                      <RefreshCw className="animate-spin" />
                    ) : (
                      <Play />
                    )}
                    {bulkAction === "start"
                      ? "启动中…"
                      : `全部启动 ${runtimeSnapshot.startableProfiles.length}`}
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => void onStopAll()}
                    disabled={
                      bulkAction !== null || runtimeSnapshot.stoppableSessions.length === 0
                    }
                  >
                    <Square />
                    {bulkAction === "stop"
                      ? "停止中…"
                      : `全部停止 ${runtimeSnapshot.stoppableSessions.length}`}
                  </Button>
                </div>
              </div>
            </CardHeader>
            <CardContent className="pt-5">
              <div className="grid gap-3 xl:grid-cols-2">
                {profiles.map((profile) => {
                  const activeSession = activeSessionForProfile(
                    sessionsById,
                    profile.profile_id,
                  );
                  const lastSessionId = lastSessionByProfile[profile.profile_id];
                  const displaySession =
                    activeSession ??
                    (lastSessionId ? sessionsById[lastSessionId] : undefined);
                  const sessionLogs = displaySession
                    ? logsBySessionId[displaySession.launch_session_id] ?? []
                    : [];

                  return (
                    <Card
                      key={profile.profile_id}
                      className="gap-3 bg-background/40 shadow-none"
                    >
                      <CardHeader className="gap-3 px-4 pt-4">
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <Badge variant="secondary">
                            {labelStatus(profile.process_role)}
                          </Badge>
                          {displaySession ? (
                            <Badge
                              variant="outline"
                              className={sessionBadgeClass(displaySession)}
                            >
                              {labelStatus(displaySession.state)} · PID {displaySession.pid ?? "—"}
                              {displaySession.exit_code !== undefined &&
                              displaySession.exit_code !== null
                                ? ` · 退出码 ${displaySession.exit_code}`
                                : ""}
                            </Badge>
                          ) : (
                            <Badge variant="outline" className="text-muted-foreground">
                              未运行
                            </Badge>
                          )}
                        </div>
                        <div className="rounded-lg border border-border bg-muted/35 px-3 py-2.5 font-mono text-xs leading-relaxed text-foreground">
                          {profile.command}
                        </div>
                        <CardDescription
                          className="truncate font-mono text-xs"
                          title={profile.working_directory}
                        >
                          {formatDisplayPath(profile.working_directory)}
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="mt-auto space-y-3 px-4 pb-4">
                        <div className="flex flex-wrap gap-2">
                          {activeSession ? (
                            <Button
                              size="sm"
                              variant="outline"
                              onClick={() => onStop(activeSession)}
                              disabled={
                                bulkAction !== null || activeSession.state === "STOPPING"
                              }
                            >
                              <Square />
                              {activeSession.state === "STOPPING" ? "停止中…" : "停止"}
                            </Button>
                          ) : (
                            <>
                              <Button
                                size="sm"
                                onClick={() => onStart(profile)}
                                disabled={bulkAction !== null}
                              >
                                <Play />
                                启动
                              </Button>
                              <Button
                                size="sm"
                                variant="ghost"
                                className="text-muted-foreground hover:text-destructive"
                                onClick={() => void onRemoveProfile(profile)}
                                disabled={bulkAction !== null}
                              >
                                <Trash2 />
                                移除
                              </Button>
                            </>
                          )}
                        </div>
                        {displaySession && (
                          <div className="overflow-hidden rounded-lg border border-border bg-[var(--log-bg)]">
                            <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-xs text-muted-foreground">
                              <Terminal className="size-3.5" />
                              会话输出
                            </div>
                            <pre className="max-h-52 overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-xs leading-relaxed text-foreground">
                              {sessionLogs.join("\n") || "暂无输出"}
                            </pre>
                          </div>
                        )}
                      </CardContent>
                    </Card>
                  );
                })}
              </div>
            </CardContent>
          </Card>
        )}
      </div>
    </>
  );
}
