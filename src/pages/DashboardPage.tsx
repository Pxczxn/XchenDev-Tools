import { useCallback, useEffect, useRef, useState } from "react";
import { PageHeader } from "../components/PageHeader";
import { healthCheck, listRecentErrors, listRuntimeItems } from "../ipc/client";
import type { HealthCheckResponse, RecentError, RuntimeItem } from "../ipc/types";
import { labelPlatform, labelStatus } from "../lib/statusLabels";

type LoadState = "loading" | "success" | "error";

function runtimeStateClass(state: string): string {
  const s = state.toUpperCase();
  if (s === "RUNNING") return "env-badge ok";
  if (["STARTING", "STOPPING"].includes(s)) return "env-badge loading";
  if (["FAILED", "BLOCKED"].includes(s)) return "env-badge err";
  if (["CONFIGURED", "DISCOVERED", "STOPPED"].includes(s)) return "env-badge muted";
  return "env-badge muted";
}

export function DashboardPage() {
  const [state, setState] = useState<LoadState>("loading");
  const [health, setHealth] = useState<HealthCheckResponse | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [runtimeItems, setRuntimeItems] = useState<RuntimeItem[]>([]);
  const [recentErrors, setRecentErrors] = useState<RecentError[]>([]);
  const refreshInFlightRef = useRef(false);

  const refreshDashboard = useCallback(async (showLoading: boolean) => {
    if (refreshInFlightRef.current) return;
    refreshInFlightRef.current = true;
    if (showLoading) setState("loading");
    setErrorCode(null);
    try {
      const [data, items, errors] = await Promise.all([
        healthCheck(),
        listRuntimeItems(),
        listRecentErrors(8),
      ]);
      setHealth(data);
      setRuntimeItems(items);
      setRecentErrors(errors);
      setState("success");
    } catch (e) {
      const err = e as { code?: string };
      setErrorCode(err.code ?? "UNKNOWN");
      setHealth(null);
      setRuntimeItems([]);
      setRecentErrors([]);
      setState("error");
    } finally {
      refreshInFlightRef.current = false;
    }
  }, []);

  useEffect(() => {
    void refreshDashboard(true);
    const timer = window.setInterval(() => {
      void refreshDashboard(false);
    }, 3000);
    return () => window.clearInterval(timer);
  }, [refreshDashboard]);

  async function onRefresh() {
    await refreshDashboard(true);
  }

  return (
    <>
      <PageHeader
        title="系统概览"
        description="本机运行状态、服务与最近错误一览"
        actions={
          <button
            type="button"
            className="secondary btn-sm"
            onClick={onRefresh}
            disabled={state === "loading"}
          >
            {state === "loading" ? "刷新中…" : "刷新"}
          </button>
        }
      />

      <div className="dashboard-grid">
        <section className="card">
          <div className="card-header">
            <h3>IPC 健康状态</h3>
          </div>
          <div className="card-body">
            {state === "loading" && (
              <div className="stat-grid">
                <div className="stat-item">
                  <span className="stat-label">状态</span>
                  <span className="status-pill loading">检测中…</span>
                </div>
              </div>
            )}
            {state === "success" && health && (
              <div className="stat-grid">
                <div className="stat-item">
                  <span className="stat-label">状态</span>
                  <span className="status-pill ok">
                    {labelStatus(health.ipc_status)}
                  </span>
                </div>
                <div className="stat-item">
                  <span className="stat-label">平台</span>
                  <span className="stat-value">
                    {labelPlatform(health.platform)}
                  </span>
                </div>
                <div className="stat-item">
                  <span className="stat-label">版本</span>
                  <span className="stat-value mono">{health.app_version}</span>
                </div>
              </div>
            )}
            {state === "error" && (
              <div className="stat-grid">
                <div className="stat-item">
                  <span className="stat-label">状态</span>
                  <span className="status-pill err">
                    {labelStatus(errorCode)}
                  </span>
                </div>
                <div className="stat-item stat-item-wide">
                  <span className="stat-label">说明</span>
                  <span className="stat-value muted-text">
                    IPC 不可用，请检查后端连接后重试。
                  </span>
                </div>
                <div className="stat-item">
                  <button type="button" className="btn-sm" onClick={onRefresh}>
                    重试
                  </button>
                </div>
              </div>
            )}
          </div>
        </section>

        <section className="card">
          <div className="card-header">
            <h3>运行项</h3>
            <span className="card-meta">{runtimeItems.length} 项</span>
          </div>
          <div className="card-body">
            {runtimeItems.length === 0 && (
              <div className="empty compact">
                暂无运行项。完成环境或项目配置后可在此查看状态。
              </div>
            )}
            {runtimeItems.length > 0 && (
              <div className="env-candidate-list">
                {runtimeItems.map((item) => (
                  <div key={item.id} className="env-candidate">
                    <div className="env-candidate-head">
                      <span className={runtimeStateClass(item.state)}>
                        {labelStatus(item.state)}
                      </span>
                      <span className="env-source">
                        {labelStatus(item.runtime_mode)}
                      </span>
                    </div>
                    <div className="env-path">{item.name}</div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>
      </div>

      {recentErrors.length > 0 && (
        <section className="card">
          <div className="card-header">
            <h3>最近错误</h3>
            <span className="card-meta">{recentErrors.length} 条</span>
          </div>
          <div className="card-body">
            <div className="env-candidate-list">
              {recentErrors.map((e, i) => (
                <div key={`${e.timestamp}-${i}`} className="env-candidate">
                  <div className="env-candidate-head">
                    <span className="env-badge err">{labelStatus(e.code)}</span>
                    <span className="env-source">{e.source}</span>
                  </div>
                  <div className="env-reason">{e.message}</div>
                </div>
              ))}
            </div>
          </div>
        </section>
      )}
    </>
  );
}
