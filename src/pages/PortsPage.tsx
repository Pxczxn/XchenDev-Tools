import { useState } from "react";
import { PageHeader } from "../components/PageHeader";
import {
  inspectPort,
  issueProcessTerminationConfirmation,
  terminateProcess,
} from "../ipc/client";
import type { PortOccupancy } from "../ipc/types";
import { formatDisplayPath } from "../lib/formatDisplay";
import {
  formatOperationMessage,
  labelErrorText,
} from "../lib/statusLabels";

export function PortsPage() {
  const [port, setPort] = useState("3000");
  const [rows, setRows] = useState<PortOccupancy[]>([]);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function onQuery() {
    setLoading(true);
    setMessage(null);
    try {
      const p = Number(port);
      const data = await inspectPort("both", p);
      setRows(data);
    } catch (e) {
      setMessage(labelErrorText(String(e)));
      setRows([]);
    } finally {
      setLoading(false);
    }
  }

  async function onTerminate(row: PortOccupancy, force: boolean) {
    if (row.protection.is_protected) return;
    const mode = force ? "force" : "normal";
    try {
      const confirmation = await issueProcessTerminationConfirmation({
        pid: row.process.pid,
        mode,
        expectedName: row.process.name,
        expectedCwd: row.process.working_directory,
      });
      const ok = window.confirm(`确认${force ? "强制" : ""}终止？\n${confirmation.binding_summary}`);
      if (!ok) return;

      const result = await terminateProcess({
        pid: row.process.pid,
        mode,
        confirmationToken: confirmation.confirmation_token,
        expectedName: row.process.name,
        expectedCwd: row.process.working_directory,
      });
      setMessage(
        formatOperationMessage(
          result.status,
          result.message,
          result.reason_code,
        ),
      );
      await onQuery();
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  return (
    <>
      <PageHeader
        title="端口管理"
        description="同时查询 TCP / UDP 端口占用，并在确认后安全终止"
      />
      <div className="card toolbar-card env-toolbar">
        <input
          className="input-port"
          value={port}
          onChange={(e) => setPort(e.target.value)}
          placeholder="端口"
        />
        <button type="button" onClick={onQuery} disabled={loading}>
          {loading ? "查询中…" : "查询"}
        </button>
      </div>
      {message && <div className="feedback-banner">{message}</div>}
      {rows.length === 0 && !loading && (
        <div className="card">
          <div className="empty">无占用或未查询。</div>
        </div>
      )}
      {rows.length > 0 && (
        <div className="env-candidate-list">
          {rows.map((r, i) => (
            <div key={i} className="card env-candidate">
              <div className="env-candidate-head">
                <span className="env-source">
                  {r.protocol.toUpperCase()} :{r.port}
                </span>
                <span className="env-tag">PID {r.process.pid}</span>
                {r.protection.is_protected ? (
                  <span className="env-badge err">受保护</span>
                ) : (
                  <span className="env-badge ok">可终止</span>
                )}
              </div>
              <div className="env-path">{r.process.name}</div>
              <div className="env-meta">
                <span className="env-meta-label">监听</span>
                <span className="env-version">{r.listen_address}</span>
              </div>
              {r.process.working_directory && (
                <div className="env-meta">
                  <span className="env-meta-label">目录</span>
                  <span className="env-version" title={r.process.working_directory}>
                    {formatDisplayPath(r.process.working_directory)}
                  </span>
                </div>
              )}
              {r.protection.is_protected && r.protection.reason && (
                <div className="env-reason">{r.protection.reason}</div>
              )}
              <div className="list-card-actions">
                <button
                  type="button"
                  disabled={r.protection.is_protected}
                  onClick={() => onTerminate(r, false)}
                >
                  终止
                </button>
                <button
                  type="button"
                  className="secondary"
                  disabled={r.protection.is_protected}
                  onClick={() => onTerminate(r, true)}
                >
                  强制终止
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
