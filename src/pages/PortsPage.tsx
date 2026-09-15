import { useRef, useState } from "react";
import { PageHeader } from "../components/PageHeader";
import {
  inspectPort,
  issueProcessTerminationConfirmation,
  terminatePortProcess,
} from "../ipc/client";
import type { PortOccupancy } from "../ipc/types";
import { formatDisplayPath } from "../lib/formatDisplay";
import {
  formatOperationMessage,
  labelErrorText,
} from "../lib/statusLabels";

function parsePort(value: string): number | null {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) return null;
  return parsed;
}

export function PortsPage() {
  const [port, setPort] = useState("3000");
  const [rows, setRows] = useState<PortOccupancy[]>([]);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const requestGenerationRef = useRef(0);

  function invalidatePendingQuery() {
    requestGenerationRef.current += 1;
    setLoading(false);
  }

  function changePort(value: string) {
    invalidatePendingQuery();
    setPort(value);
    setRows([]);
    setMessage(null);
  }

  async function refreshRows(clearMessage: boolean): Promise<boolean> {
    const generation = requestGenerationRef.current + 1;
    requestGenerationRef.current = generation;
    const parsedPort = parsePort(port);
    if (parsedPort === null) {
      setRows([]);
      setMessage("端口必须是 1~65535 的整数");
      setLoading(false);
      return false;
    }

    setLoading(true);
    if (clearMessage) setMessage(null);
    try {
      const data = await inspectPort("both", parsedPort);
      if (generation !== requestGenerationRef.current) return false;
      setRows(data);
      return true;
    } catch (e) {
      if (generation !== requestGenerationRef.current) return false;
      setMessage(labelErrorText(String(e)));
      setRows([]);
      return false;
    } finally {
      if (generation === requestGenerationRef.current) {
        setLoading(false);
      }
    }
  }

  async function onQuery() {
    await refreshRows(true);
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

      const result = await terminatePortProcess({
        pid: row.process.pid,
        protocol: row.protocol,
        port: row.port,
        snapshotDigest: row.snapshot_digest,
        mode,
        confirmationToken: confirmation.confirmation_token,
        expectedName: row.process.name,
        expectedCwd: row.process.working_directory,
      });
      const operationMessage = formatOperationMessage(
        result.status,
        result.message,
        result.reason_code,
      );
      if (await refreshRows(false)) {
        setMessage(operationMessage);
      }
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
          onChange={(e) => changePort(e.target.value)}
          placeholder="端口"
          inputMode="numeric"
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