from pathlib import Path
import json


def write(path: str, content: str) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8", newline="\n")


def replace_exact(path: str, old: str, new: str, count: int = 1) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    actual = text.count(old)
    if actual != count:
        raise SystemExit(f"{path}: expected {count} match(es), found {actual}")
    target.write_text(text.replace(old, new, count), encoding="utf-8", newline="\n")


# shadcn/ui project metadata.
write(
    "components.json",
    json.dumps(
        {
            "$schema": "https://ui.shadcn.com/schema.json",
            "style": "new-york",
            "rsc": False,
            "tsx": True,
            "tailwind": {
                "config": "",
                "css": "src/styles/shadcn.css",
                "baseColor": "neutral",
                "cssVariables": True,
                "prefix": "",
            },
            "iconLibrary": "lucide",
            "aliases": {
                "components": "@/components",
                "utils": "@/lib/utils",
                "ui": "@/components/ui",
                "lib": "@/lib",
                "hooks": "@/hooks",
            },
        },
        ensure_ascii=False,
        indent=2,
    ) + "\n",
)

# Tailwind v4 utilities only: no Preflight, so existing pages keep their current reset/base behavior.
write(
    "src/styles/shadcn.css",
    '''@layer theme, base, components, utilities;
@import "tailwindcss/theme.css" layer(theme);
@import "tailwindcss/utilities.css" layer(utilities);

:root {
  --sh-background: var(--bg);
  --sh-foreground: var(--text);
  --sh-card: var(--surface);
  --sh-card-foreground: var(--text-strong);
  --sh-primary: var(--accent);
  --sh-primary-foreground: #fff;
  --sh-secondary: var(--secondary-btn-bg);
  --sh-secondary-foreground: var(--text);
  --sh-muted: var(--surface-raised);
  --sh-muted-foreground: var(--muted);
  --sh-accent: var(--nav-active-bg);
  --sh-accent-foreground: var(--text-strong);
  --sh-destructive: var(--danger);
  --sh-border: var(--border);
  --sh-input: var(--border);
  --sh-ring: var(--accent);
  --sh-radius: 0.625rem;
}

@theme inline {
  --color-background: var(--sh-background);
  --color-foreground: var(--sh-foreground);
  --color-card: var(--sh-card);
  --color-card-foreground: var(--sh-card-foreground);
  --color-primary: var(--sh-primary);
  --color-primary-foreground: var(--sh-primary-foreground);
  --color-secondary: var(--sh-secondary);
  --color-secondary-foreground: var(--sh-secondary-foreground);
  --color-muted: var(--sh-muted);
  --color-muted-foreground: var(--sh-muted-foreground);
  --color-accent: var(--sh-accent);
  --color-accent-foreground: var(--sh-accent-foreground);
  --color-destructive: var(--sh-destructive);
  --color-border: var(--sh-border);
  --color-input: var(--sh-input);
  --color-ring: var(--sh-ring);
  --radius-sm: calc(var(--sh-radius) - 4px);
  --radius-md: calc(var(--sh-radius) - 2px);
  --radius-lg: var(--sh-radius);
  --radius-xl: calc(var(--sh-radius) + 4px);
}
''',
)

write(
    "src/lib/utils.ts",
    '''import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
''',
)

write(
    "src/components/ui/button.tsx",
    '''import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors outline-none disabled:pointer-events-none disabled:opacity-50 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary/90",
        destructive:
          "bg-destructive text-white hover:bg-destructive/90 focus-visible:ring-destructive/30",
        outline:
          "border border-input bg-background text-foreground hover:bg-accent hover:text-accent-foreground",
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        ghost: "text-foreground hover:bg-accent hover:text-accent-foreground",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-10 rounded-md px-6",
        icon: "size-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant,
  size,
  type = "button",
  ...props
}: React.ComponentProps<"button"> & VariantProps<typeof buttonVariants>) {
  return (
    <button
      data-slot="button"
      type={type}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
''',
)

write(
    "src/components/ui/card.tsx",
    '''import * as React from "react";
import { cn } from "@/lib/utils";

function Card({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card"
      className={cn(
        "flex flex-col gap-5 rounded-xl border border-border bg-card text-card-foreground shadow-sm",
        className,
      )}
      {...props}
    />
  );
}

function CardHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-header"
      className={cn("grid gap-1.5 px-5 pt-5", className)}
      {...props}
    />
  );
}

function CardTitle({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-title"
      className={cn("font-semibold leading-none tracking-tight", className)}
      {...props}
    />
  );
}

function CardDescription({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-description"
      className={cn("text-sm text-muted-foreground", className)}
      {...props}
    />
  );
}

function CardContent({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-content"
      className={cn("px-5 pb-5", className)}
      {...props}
    />
  );
}

function CardFooter({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-footer"
      className={cn("flex items-center px-5 pb-5", className)}
      {...props}
    />
  );
}

export { Card, CardHeader, CardTitle, CardDescription, CardContent, CardFooter };
''',
)

write(
    "src/components/ui/badge.tsx",
    '''import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex w-fit shrink-0 items-center justify-center gap-1 rounded-md border px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-colors [&_svg]:size-3",
  {
    variants: {
      variant: {
        default: "border-transparent bg-primary text-primary-foreground",
        secondary:
          "border-transparent bg-secondary text-secondary-foreground",
        destructive: "border-transparent bg-destructive text-white",
        outline: "border-border text-foreground",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return (
    <span
      data-slot="badge"
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  );
}

export { Badge, badgeVariants };
''',
)

write(
    "src/components/ui/input.tsx",
    '''import * as React from "react";
import { cn } from "@/lib/utils";

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      data-slot="input"
      type={type}
      className={cn(
        "h-9 w-full min-w-0 rounded-md border border-input bg-background px-3 py-1 text-sm text-foreground shadow-xs outline-none transition-colors placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
''',
)

# TypeScript alias used by shadcn generated components.
tsconfig = json.loads(Path("tsconfig.json").read_text(encoding="utf-8"))
compiler = tsconfig.setdefault("compilerOptions", {})
compiler["baseUrl"] = "."
compiler["paths"] = {"@/*": ["./src/*"]}
write("tsconfig.json", json.dumps(tsconfig, ensure_ascii=False, indent=2) + "\n")

# Vite + Tailwind v4 plugin + @ alias.
write(
    "vite.config.ts",
    '''import path from "node:path";
import process from "node:process";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "node",
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
''',
)

replace_exact(
    "src/layout/AppLayout.tsx",
    'import "../styles/app.css";',
    'import "../styles/app.css";\nimport "../styles/shadcn.css";',
)

# Remove the temporary handcrafted Projects-only CSS now replaced by shadcn/Tailwind layout.
app_css_path = Path("src/styles/app.css")
app_css = app_css_path.read_text(encoding="utf-8")
start_marker = ".project-runtime-actions {"
end_marker = ".toolbar-card {"
start = app_css.find(start_marker)
end = app_css.find(end_marker, start)
if start < 0 or end < 0:
    raise SystemExit("Projects custom CSS cleanup markers not found")
app_css_path.write_text(app_css[:start] + app_css[end:], encoding="utf-8", newline="\n")

# shadcn imports + Lucide icons.
replace_exact(
    "src/pages/ProjectsPage.tsx",
    'import { PageHeader } from "../components/PageHeader";',
    '''import {
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
import { Input } from "../components/ui/input";''',
)

replace_exact(
    "src/pages/ProjectsPage.tsx",
    '''function sessionBadgeClass(session: LaunchSessionInfo): string {
  if (ACTIVE_SESSION_STATES.has(session.state)) return "env-badge ok";
  if (session.state === "FAILED" || (session.exit_code ?? 0) !== 0) {
    return "env-badge err";
  }
  return "env-badge muted";
}''',
    '''function sessionBadgeClass(session: LaunchSessionInfo): string {
  if (ACTIVE_SESSION_STATES.has(session.state)) {
    return "border-emerald-500/25 bg-emerald-500/10 text-emerald-500";
  }
  if (session.state === "FAILED" || (session.exit_code ?? 0) !== 0) {
    return "border-destructive/25 bg-destructive/10 text-destructive";
  }
  return "border-border bg-muted text-muted-foreground";
}''',
)

page_path = Path("src/pages/ProjectsPage.tsx")
page = page_path.read_text(encoding="utf-8")
return_marker = "  return (\n    <>\n"
start = page.rfind(return_marker)
if start < 0:
    raise SystemExit("ProjectsPage return marker not found")

new_return = '''  return (
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
                              {sessionLogs.join("\\n") || "暂无输出"}
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
'''
page_path.write_text(page[:start] + new_return, encoding="utf-8", newline="\n")
