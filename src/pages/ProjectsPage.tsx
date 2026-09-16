import {
  Activity,
  AlertTriangle,
  Boxes,
  CheckCircle2,
  FolderKanban,
  FolderOpen,
  Play,
  Plus,
  RefreshCw,
  Save,
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
import { useProjectManager } from "../features/projects/useProjectManager";
import { displaySessionForProfile } from "../features/projects/model";
import type { LaunchSessionInfo } from "../ipc/types";
import { formatDisplayPath } from "../lib/formatDisplay";
import { labelStatus } from "../lib/statusLabels";

function sessionBadgeClass(session: LaunchSessionInfo): string {
  if (["STARTING", "RUNNING", "STOPPING"].includes(session.state)) {
    return "border-emerald-500/25 bg-emerald-500/10 text-emerald-500";
  }
  if (session.state === "FAILED" || (session.exit_code ?? 0) !== 0) {
    return "border-destructive/25 bg-destructive/10 text-destructive";
  }
  return "border-border bg-muted text-muted-foreground";
}

export function ProjectsPage() {
  const manager = useProjectManager();
  const activeView =
    manager.view === "overview"
      ? manager.profiles.length > 0
        ? "runtime"
        : "scanner"
      : manager.view;

  return (
    <>
      <PageHeader
        title="项目管理"
        description="选择项目，向下扫描两层目录，配置启动命令并统一运行。"
      />

      <div className="grid items-start gap-4 xl:grid-cols-[280px_minmax(0,1fr)]">
        <Card className="xl:sticky xl:top-4">
          <CardHeader className="gap-3">
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-center gap-3">
                <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                  <FolderKanban className="size-4" />
                </div>
                <div className="min-w-0">
                  <CardTitle>项目库</CardTitle>
                  <CardDescription>{manager.projects.length} 个已保存项目</CardDescription>
                </div>
              </div>
              <Button
                size="icon"
                variant="outline"
                onClick={() => void manager.pickDirectory()}
                aria-label="添加项目目录"
              >
                <Plus />
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-2">
            {manager.projects.length === 0 ? (
              <button
                data-slot="button"
                type="button"
                className="w-full rounded-lg border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground transition-colors hover:border-primary/40 hover:bg-muted/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                onClick={() => void manager.pickDirectory()}
              >
                <FolderOpen className="mx-auto mb-3 size-5" />
                选择一个项目目录开始
              </button>
            ) : (
              manager.projects.map((project) => {
                const active = manager.projectId === project.project_id;
                return (
                  <div
                    key={project.project_id}
                    className={
                      active
                        ? "rounded-lg border border-primary/50 bg-primary/[0.05] p-3 ring-1 ring-primary/10"
                        : "rounded-lg border border-border bg-background/40 p-3 transition-colors hover:border-primary/30"
                    }
                  >
                    <div className="flex items-start justify-between gap-2">
                      <button
                        data-slot="button"
                        type="button"
                        className="min-w-0 flex-1 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                        onClick={() => void manager.openProject(project)}
                      >
                        <div className="flex items-center gap-2">
                          <span className="truncate text-sm font-medium text-foreground">
                            {project.name}
                          </span>
                          {active && <Badge className="shrink-0">当前</Badge>}
                        </div>
                        <div
                          className="mt-1 truncate font-mono text-[11px] text-muted-foreground"
                          title={project.root_path}
                        >
                          {formatDisplayPath(project.root_path)}
                        </div>
                      </button>
                      <Button
                        size="icon-sm"
                        variant="ghost"
                        className="shrink-0 text-muted-foreground hover:text-destructive"
                        onClick={() => void manager.removeSavedProject(project)}
                        aria-label={`移除项目 ${project.name}`}
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </div>
                );
              })
            )}
          </CardContent>
        </Card>

        <div className="min-w-0 space-y-4">
          <Card>
            <CardHeader className="gap-4">
              <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                <div className="min-w-0">
                  <CardTitle className="flex flex-wrap items-center gap-2">
                    {manager.currentProject?.name ?? (manager.rootPath ? "新项目" : "项目工作台")}
                    {manager.currentProject && <Badge variant="secondary">已保存</Badge>}
                  </CardTitle>
                  <CardDescription className="mt-1">
                    扫描根目录以及向下两层目录，只识别启动相关项目文件，不自动执行命令。
                  </CardDescription>
                </div>
                {manager.rootPath && (
                  <div className="flex flex-wrap gap-2">
                    <Badge variant="outline">启动配置 {manager.stats.profiles}</Badge>
                    <Badge
                      variant="outline"
                      className={
                        manager.stats.activeSessions > 0
                          ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-500"
                          : undefined
                      }
                    >
                      运行 {manager.stats.activeSessions}
                    </Badge>
                    {manager.stats.failedSessions > 0 && (
                      <Badge
                        variant="outline"
                        className="border-destructive/25 bg-destructive/10 text-destructive"
                      >
                        失败 {manager.stats.failedSessions}
                      </Badge>
                    )}
                  </div>
                )}
              </div>

              <div className="grid gap-2 lg:grid-cols-[minmax(0,1fr)_auto_auto]">
                <div className="relative min-w-0">
                  <FolderOpen className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={formatDisplayPath(manager.rootPath)}
                    onChange={(event) => manager.editRootPath(event.target.value)}
                    className="pl-9 font-mono text-xs"
                    placeholder="选择或输入项目根目录"
                  />
                </div>
                <Button variant="outline" onClick={() => void manager.pickDirectory()}>
                  <FolderOpen />
                  选择目录
                </Button>
                <Button
                  onClick={() => void manager.scanProject()}
                  disabled={!manager.rootPath || manager.loading}
                >
                  {manager.loading ? (
                    <RefreshCw className="animate-spin" />
                  ) : (
                    <ScanSearch />
                  )}
                  {manager.loading
                    ? "扫描中…"
                    : manager.projectId
                      ? "重新扫描"
                      : "扫描并添加"}
                </Button>
              </div>
            </CardHeader>
          </Card>

          {manager.notice && (
            <div
              className={
                manager.notice.tone === "error"
                  ? "flex items-start gap-2 rounded-lg border border-destructive/25 bg-destructive/10 px-4 py-3 text-sm text-destructive"
                  : manager.notice.tone === "success"
                    ? "flex items-start gap-2 rounded-lg border border-emerald-500/25 bg-emerald-500/10 px-4 py-3 text-sm text-emerald-600 dark:text-emerald-400"
                    : "flex items-start gap-2 rounded-lg border border-border bg-muted/40 px-4 py-3 text-sm text-muted-foreground"
              }
            >
              {manager.notice.tone === "error" ? (
                <AlertTriangle className="mt-0.5 size-4 shrink-0" />
              ) : (
                <CheckCircle2 className="mt-0.5 size-4 shrink-0" />
              )}
              <span>{manager.notice.text}</span>
            </div>
          )}

          {!manager.rootPath ? (
            <Card>
              <CardContent className="flex min-h-72 flex-col items-center justify-center px-6 py-12 text-center">
                <div className="mb-4 flex size-12 items-center justify-center rounded-xl bg-muted text-muted-foreground">
                  <FolderKanban className="size-5" />
                </div>
                <h3 className="text-base font-semibold text-foreground">先选择一个项目</h3>
                <p className="mt-2 max-w-md text-sm leading-6 text-muted-foreground">
                  从左侧打开已保存项目，或者选择一个新目录。扫描只向下检查两层目录，不会执行任何发现的命令。
                </p>
                <Button className="mt-5" onClick={() => void manager.pickDirectory()}>
                  <Plus />
                  选择项目目录
                </Button>
              </CardContent>
            </Card>
          ) : (
            <>
              <div className="flex items-center gap-1 rounded-lg border border-border bg-muted/35 p-1">
                <Button
                  size="sm"
                  variant={activeView === "scanner" ? "secondary" : "ghost"}
                  className="flex-1 sm:flex-none"
                  onClick={() => manager.setView("scanner")}
                >
                  <Settings2 />
                  启动配置
                  {manager.candidates.length > 0 && (
                    <Badge variant="outline" className="ml-1">
                      {manager.candidates.length}
                    </Badge>
                  )}
                </Button>
                <Button
                  size="sm"
                  variant={activeView === "runtime" ? "secondary" : "ghost"}
                  className="flex-1 sm:flex-none"
                  onClick={() => manager.setView("runtime")}
                >
                  <Activity />
                  运行控制
                  {manager.profiles.length > 0 && (
                    <Badge variant="outline" className="ml-1">
                      {manager.profiles.length}
                    </Badge>
                  )}
                </Button>
              </div>

              {activeView === "scanner" ? (
                <div className="grid items-start gap-4 2xl:grid-cols-[minmax(0,1.15fr)_minmax(360px,0.85fr)]">
                  <Card>
                    <CardHeader>
                      <div className="flex items-start justify-between gap-3">
                        <div>
                          <CardTitle className="flex items-center gap-2">
                            <Boxes className="size-4 text-primary" />
                            扫描结果
                          </CardTitle>
                          <CardDescription className="mt-1">
                            找到启动相关文件后，选一项到右侧配置命令。
                          </CardDescription>
                        </div>
                        {manager.stats.conflicts > 0 && (
                          <Badge
                            variant="outline"
                            className="border-destructive/25 bg-destructive/10 text-destructive"
                          >
                            {manager.stats.conflicts} 个冲突
                          </Badge>
                        )}
                      </div>
                    </CardHeader>
                    <CardContent>
                      {manager.loading ? (
                        <div className="flex min-h-48 items-center justify-center gap-2 text-sm text-muted-foreground">
                          <RefreshCw className="size-4 animate-spin" />
                          正在扫描两层目录…
                        </div>
                      ) : manager.candidates.length === 0 ? (
                        <div className="flex min-h-48 flex-col items-center justify-center rounded-lg border border-dashed border-border px-5 text-center">
                          <ScanSearch className="mb-3 size-5 text-muted-foreground" />
                          <p className="text-sm font-medium text-foreground">还没有扫描结果</p>
                          <p className="mt-1 text-xs text-muted-foreground">
                            点击上方“{manager.projectId ? "重新扫描" : "扫描并添加"}”开始识别。
                          </p>
                        </div>
                      ) : (
                        <div className="space-y-2">
                          {manager.candidates.map((candidate) => {
                            const selected = manager.selectedCandidate?.id === candidate.id;
                            return (
                              <button
                                data-slot="button"
                                key={candidate.id}
                                type="button"
                                className={
                                  selected
                                    ? "w-full rounded-lg border border-primary/50 bg-primary/[0.05] p-3 text-left ring-1 ring-primary/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                                    : "w-full rounded-lg border border-border bg-background/40 p-3 text-left transition-colors hover:border-primary/30 hover:bg-muted/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                                }
                                onClick={() => manager.selectCandidate(candidate)}
                              >
                                <div className="flex flex-wrap items-center gap-2">
                                  <Badge variant="secondary">{labelStatus(candidate.stack)}</Badge>
                                  <Badge
                                    variant="outline"
                                    className={
                                      candidate.status === "CONFLICT"
                                        ? "border-destructive/25 bg-destructive/10 text-destructive"
                                        : "text-muted-foreground"
                                    }
                                  >
                                    {labelStatus(candidate.status)}
                                  </Badge>
                                  <span
                                    className="ml-auto max-w-full truncate font-mono text-[11px] text-muted-foreground"
                                    title={candidate.directory}
                                  >
                                    {formatDisplayPath(candidate.directory)}
                                  </span>
                                </div>
                                <div className="mt-2 flex flex-col gap-1 text-xs text-muted-foreground">
                                  <span className="truncate" title={candidate.evidence_file}>
                                    {formatDisplayPath(candidate.evidence_file)}
                                  </span>
                                  {candidate.suggested_command && (
                                    <code className="truncate rounded bg-muted/50 px-2 py-1.5 text-foreground">
                                      {candidate.suggested_command}
                                    </code>
                                  )}
                                </div>
                              </button>
                            );
                          })}
                        </div>
                      )}
                    </CardContent>
                  </Card>

                  <Card className="2xl:sticky 2xl:top-4">
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2">
                        <Settings2 className="size-4 text-primary" />
                        配置启动命令
                      </CardTitle>
                      <CardDescription>
                        {manager.selectedCandidate
                          ? "扫描结果只是建议，保存前可以直接修改。"
                          : "先从左侧选择一个扫描结果。"}
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-4">
                      {manager.selectedCandidate ? (
                        <>
                          <div className="space-y-1.5">
                            <label className="text-xs font-medium text-muted-foreground">
                              进程角色
                            </label>
                            <select
                              className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none focus:border-ring focus:ring-2 focus:ring-ring/40"
                              value={manager.role}
                              onChange={(event) =>
                                manager.setRole(event.target.value as "frontend" | "backend")
                              }
                            >
                              <option value="frontend">前端</option>
                              <option value="backend">后端</option>
                            </select>
                          </div>
                          <div className="space-y-1.5">
                            <label className="text-xs font-medium text-muted-foreground">
                              工作目录
                            </label>
                            <Input
                              value={manager.workdir}
                              onChange={(event) => manager.setWorkdir(event.target.value)}
                              className="font-mono text-xs"
                              placeholder="启动命令执行目录"
                            />
                          </div>
                          <div className="space-y-1.5">
                            <label className="text-xs font-medium text-muted-foreground">
                              启动命令
                            </label>
                            <Input
                              value={manager.command}
                              onChange={(event) => manager.setCommand(event.target.value)}
                              className="font-mono text-xs"
                              placeholder="例如 npm run dev"
                            />
                            {manager.selectedCandidate.scripts &&
                              manager.selectedCandidate.scripts.length > 0 && (
                                <div className="flex flex-wrap gap-1.5 pt-1">
                                  {manager.selectedCandidate.scripts.slice(0, 6).map((script) => (
                                    <button
                                      data-slot="button"
                                      key={script}
                                      type="button"
                                      className="rounded-md border border-border bg-muted/30 px-2 py-1 font-mono text-[11px] text-muted-foreground transition-colors hover:border-primary/30 hover:bg-muted/60 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                                      onClick={() => manager.setCommand(`npm run ${script}`)}
                                    >
                                      npm run {script}
                                    </button>
                                  ))}
                                </div>
                              )}
                          </div>
                          <div className="flex gap-2 pt-1">
                            <Button
                              className="flex-1"
                              onClick={() => void manager.saveProfile()}
                              disabled={
                                !manager.projectId ||
                                !manager.workdir.trim() ||
                                !manager.command.trim()
                              }
                            >
                              <Save />
                              保存启动配置
                            </Button>
                            <Button variant="outline" onClick={manager.clearComposer}>
                              取消
                            </Button>
                          </div>
                        </>
                      ) : (
                        <div className="flex min-h-48 flex-col items-center justify-center rounded-lg border border-dashed border-border px-5 text-center text-sm text-muted-foreground">
                          <Settings2 className="mb-3 size-5" />
                          点击扫描结果即可配置
                        </div>
                      )}
                    </CardContent>
                  </Card>
                </div>
              ) : (
                <Card>
                  <CardHeader className="border-b border-border pb-4">
                    <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                      <div>
                        <CardTitle className="flex items-center gap-2">
                          <Activity className="size-4 text-primary" />
                          运行控制
                        </CardTitle>
                        <CardDescription className="mt-1">
                          保存好的启动配置都在这里，扫描与运行互不干扰。
                        </CardDescription>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Badge
                          variant="outline"
                          className={
                            manager.runtimeSnapshot.activeSessions.length > 0
                              ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-500"
                              : undefined
                          }
                        >
                          运行 {manager.runtimeSnapshot.activeSessions.length}/{manager.profiles.length}
                        </Badge>
                        <Button
                          size="sm"
                          onClick={() => void manager.startAll()}
                          disabled={
                            manager.bulkAction !== null ||
                            manager.runtimeSnapshot.startableProfiles.length === 0
                          }
                        >
                          {manager.bulkAction === "start" ? (
                            <RefreshCw className="animate-spin" />
                          ) : (
                            <Play />
                          )}
                          全部启动 {manager.runtimeSnapshot.startableProfiles.length}
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => void manager.stopAll()}
                          disabled={
                            manager.bulkAction !== null ||
                            manager.runtimeSnapshot.stoppableSessions.length === 0
                          }
                        >
                          <Square />
                          全部停止 {manager.runtimeSnapshot.stoppableSessions.length}
                        </Button>
                      </div>
                    </div>
                  </CardHeader>
                  <CardContent className="pt-5">
                    {manager.profiles.length === 0 ? (
                      <div className="flex min-h-56 flex-col items-center justify-center rounded-lg border border-dashed border-border px-5 text-center">
                        <Terminal className="mb-3 size-5 text-muted-foreground" />
                        <p className="text-sm font-medium text-foreground">还没有启动配置</p>
                        <p className="mt-1 text-xs text-muted-foreground">
                          去“启动配置”扫描项目并保存一条命令。
                        </p>
                        <Button
                          size="sm"
                          variant="outline"
                          className="mt-4"
                          onClick={() => manager.setView("scanner")}
                        >
                          <Settings2 />
                          去配置
                        </Button>
                      </div>
                    ) : (
                      <div className="grid gap-3 2xl:grid-cols-2">
                        {manager.profiles.map((profile) => {
                          const activeSession = manager.getActiveSession(profile.profile_id);
                          const displaySession = displaySessionForProfile(
                            manager.sessionsById,
                            manager.lastSessionByProfile,
                            profile.profile_id,
                          );
                          const logs = displaySession
                            ? manager.logsBySessionId[displaySession.launch_session_id] ?? []
                            : [];

                          return (
                            <div
                              key={profile.profile_id}
                              className="overflow-hidden rounded-xl border border-border bg-background/40"
                            >
                              <div className="space-y-3 p-4">
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
                                        ? ` · 退出 ${displaySession.exit_code}`
                                        : ""}
                                    </Badge>
                                  ) : (
                                    <Badge variant="outline" className="text-muted-foreground">
                                      未运行
                                    </Badge>
                                  )}
                                </div>
                                <code className="block rounded-lg border border-border bg-muted/35 px-3 py-2.5 text-xs leading-relaxed text-foreground">
                                  {profile.command}
                                </code>
                                <p
                                  className="truncate font-mono text-[11px] text-muted-foreground"
                                  title={profile.working_directory}
                                >
                                  {formatDisplayPath(profile.working_directory)}
                                </p>
                                <div className="flex flex-wrap gap-2">
                                  {activeSession ? (
                                    <Button
                                      size="sm"
                                      variant="outline"
                                      onClick={() => void manager.stopSession(activeSession)}
                                      disabled={
                                        manager.bulkAction !== null ||
                                        activeSession.state === "STOPPING"
                                      }
                                    >
                                      <Square />
                                      {activeSession.state === "STOPPING" ? "停止中…" : "停止"}
                                    </Button>
                                  ) : (
                                    <>
                                      <Button
                                        size="sm"
                                        onClick={() => void manager.startProfile(profile)}
                                        disabled={manager.bulkAction !== null}
                                      >
                                        <Play />
                                        启动
                                      </Button>
                                      <Button
                                        size="sm"
                                        variant="ghost"
                                        className="text-muted-foreground hover:text-destructive"
                                        onClick={() => void manager.removeProfile(profile)}
                                        disabled={manager.bulkAction !== null}
                                      >
                                        <Trash2 />
                                        删除配置
                                      </Button>
                                    </>
                                  )}
                                </div>
                              </div>

                              {displaySession && (
                                <details className="border-t border-border">
                                  <summary className="cursor-pointer select-none px-4 py-2.5 text-xs text-muted-foreground hover:bg-muted/25">
                                    <span className="inline-flex items-center gap-2">
                                      <Terminal className="size-3.5" />
                                      会话输出 · {logs.length} 行
                                    </span>
                                  </summary>
                                  <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words bg-[var(--log-bg)] p-4 font-mono text-xs leading-relaxed text-foreground">
                                    {logs.join("\n") || "暂无输出"}
                                  </pre>
                                </details>
                              )}
                            </div>
                          );
                        })}
                      </div>
                    )}
                  </CardContent>
                </Card>
              )}
            </>
          )}
        </div>
      </div>
    </>
  );
}
