import { useCallback, useEffect, useState } from "react";
import { PageHeader } from "../components/PageHeader";
import {
  controlWindowsService,
  getAppSettings,
  issueServiceControlConfirmation,
  listManagedServices,
} from "../ipc/client";
import type { WindowsServiceInfo } from "../ipc/types";
import {
  formatManagedServiceKind,
  managedServicesSummary,
  missingManagedServiceKinds,
  normalizeManagedServiceKinds,
  type ManagedServiceKindId,
} from "../lib/managedServices";
import {
  formatOperationMessage,
  labelErrorText,
  labelStatus,
} from "../lib/statusLabels";

function statusClass(status: string): string {
  const s = status.toUpperCase();
  if (s === "RUNNING") return "env-badge ok";
  if (s === "STOPPED") return "env-badge muted";
  if (s === "STARTING" || s === "STOPPING") return "env-badge loading";
  return "env-badge err";
}

function MissingServiceCard({ kind }: { kind: ManagedServiceKindId }) {
  return (
    <div className="card service-missing-card">
      <div className="card-body">
        <div className="env-candidate-head">
          <span className="env-badge muted">未发现</span>
          <span className="env-tag">{formatManagedServiceKind(kind)}</span>
        </div>
        <div className="env-reason">
          本机未找到 {formatManagedServiceKind(kind)} 相关 Windows 服务。
          若为压缩包安装或仅进程方式运行，本页无法管理。
          若已注册为服务，请在「设置 → 服务名映射」填写准确名称（如{" "}
          <code>mysql=MySQL80</code>）。
        </div>
      </div>
    </div>
  );
}

export function ServicesPage() {
  const [services, setServices] = useState<WindowsServiceInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [managedKinds, setManagedKinds] = useState<string[]>(["mysql", "redis"]);

  const load = useCallback(async () => {
    setLoading(true);
    setMessage(null);
    try {
      const settings = await getAppSettings();
      const kinds = normalizeManagedServiceKinds(settings.managed_service_kinds);
      setManagedKinds(kinds);
      setServices(await listManagedServices());
    } catch (e) {
      setMessage(labelErrorText(String(e)));
      setServices([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onControl(svc: WindowsServiceInfo, action: string) {
    try {
      const confirmation = await issueServiceControlConfirmation(
        svc.service_name,
        action,
      );
      const ok = window.confirm(`确认执行服务操作？\n${confirmation.binding_summary}`);
      if (!ok) return;

      const result = await controlWindowsService({
        serviceName: svc.service_name,
        action,
        confirmationToken: confirmation.confirmation_token,
      });
      setMessage(
        formatOperationMessage(
          result.status,
          result.message,
          result.reason_code,
        ),
      );
      await load();
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  const enabledKinds = normalizeManagedServiceKinds(managedKinds);
  const missingKinds = missingManagedServiceKinds(enabledKinds, services);

  return (
    <>
      <PageHeader
        title="基础服务"
        description={`管理本机 ${managedServicesSummary(
          enabledKinds,
        )} 相关 Windows 服务；仅显示已注册为系统服务的实例`}
        actions={
          <button type="button" className="secondary btn-sm" onClick={load} disabled={loading}>
            {loading ? "刷新中…" : "刷新服务"}
          </button>
        }
      />
      {message && <div className="feedback-banner">{message}</div>}
      {services.length === 0 && missingKinds.length === 0 && !loading && (
        <div className="card">
          <div className="empty">
            未启用任何基础服务类型，请在「设置」中勾选要管理的类型。
          </div>
        </div>
      )}
      <div className="env-candidate-list">
        {services.map((svc) => (
          <div key={svc.service_name} className="card">
            <div className="card-body">
              <div className="env-candidate-head">
                <span className={statusClass(svc.status)}>
                  {labelStatus(svc.status)}
                </span>
                <span className="env-tag">{labelStatus(svc.kind)}</span>
                <span className="env-source">{svc.service_name}</span>
              </div>
              <div className="env-path">{svc.display_name}</div>
              {svc.status_reason && (
                <div className="env-reason">{svc.status_reason}</div>
              )}
              <div className="list-card-actions">
                <button
                  type="button"
                  disabled={!svc.can_control}
                  onClick={() => onControl(svc, "start")}
                >
                  启动
                </button>
                <button
                  type="button"
                  className="secondary"
                  disabled={!svc.can_control}
                  onClick={() => onControl(svc, "stop")}
                >
                  停止
                </button>
                <button
                  type="button"
                  className="secondary"
                  disabled={!svc.can_control}
                  onClick={() => onControl(svc, "restart")}
                >
                  重启
                </button>
              </div>
            </div>
          </div>
        ))}
        {missingKinds.map((kind) => (
          <MissingServiceCard key={kind} kind={kind} />
        ))}
      </div>
    </>
  );
}
