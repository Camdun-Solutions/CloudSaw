// Scanner orchestration UI (Contract 06).
//
// One self-contained modal per scan attempt:
//
//   1. Detect       — `scanner_detect` on mount; gates everything else.
//   2. Start        — calls `scanner_run_scan` and renders the initial record.
//   3. Poll progress — polls `scanner_scan_status` every 1s until terminal.
//   4. Cancel       — `scanner_cancel_scan`; the orchestrator kills the
//                     ScoutSuite child and the next poll reflects `canceled`.
//
// Progress is exposed by POLLING per Contract 06 §Constraints — there's no
// long-held IPC connection. The poll interval is 1s by default; we back off
// to 500ms while the scan is in `scanning` so the UI feels responsive when
// the user is staring at it.

import { useCallback, useEffect, useRef, useState } from "react";

import { Badge, Button, Modal } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  isTerminalScanStatus,
  type Account,
  type ScanRecord,
  type ScanStatus,
  type ScoutSuiteAvailability,
} from "@/lib/ipc";

type Props = {
  account: Account | null;
  onClose: () => void;
  onScanFinished: () => Promise<void>;
};

type Phase =
  | { kind: "detecting" }
  | { kind: "detect_result"; availability: ScoutSuiteAvailability }
  | { kind: "starting"; availability: ScoutSuiteAvailability }
  | {
      kind: "running";
      availability: ScoutSuiteAvailability;
      record: ScanRecord;
    }
  | {
      kind: "terminal";
      availability: ScoutSuiteAvailability;
      record: ScanRecord;
    };

const POLL_INTERVAL_MS = 1000;

export default function ScanProgressModal({
  account,
  onClose,
  onScanFinished,
}: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [phase, setPhase] = useState<Phase>({ kind: "detecting" });
  const [error, setError] = useState<string | null>(null);
  const [canceling, setCanceling] = useState(false);
  // Keep the timer in a ref so the cleanup callback always tears down the
  // latest scheduled poll regardless of how many renders happened.
  const pollTimer = useRef<number | null>(null);

  const stopPolling = useCallback(() => {
    if (pollTimer.current !== null) {
      window.clearTimeout(pollTimer.current);
      pollTimer.current = null;
    }
  }, []);

  useEffect(() => {
    if (!account) return;
    let cancelled = false;
    setPhase({ kind: "detecting" });
    setError(null);
    setCanceling(false);
    ipc
      .scannerDetect()
      .then((availability) => {
        if (cancelled) return;
        setPhase({ kind: "detect_result", availability });
      })
      .catch((err) => {
        if (cancelled) return;
        setError(formatError(err));
        setPhase({
          kind: "detect_result",
          availability: { status: "missing" },
        });
      });
    return () => {
      cancelled = true;
      stopPolling();
    };
  }, [account, formatError, stopPolling]);

  const schedulePoll = useCallback(
    (scanId: string, availability: ScoutSuiteAvailability) => {
      stopPolling();
      pollTimer.current = window.setTimeout(async () => {
        try {
          const next = await ipc.scannerScanStatus(scanId);
          if (isTerminalScanStatus(next.status)) {
            stopPolling();
            setPhase({ kind: "terminal", availability, record: next });
            await onScanFinished();
          } else {
            setPhase({ kind: "running", availability, record: next });
            schedulePoll(scanId, availability);
          }
        } catch (err) {
          // A polling error is non-fatal; surface it but keep polling so
          // the UI eventually catches up when SQLite recovers.
          setError(formatError(err));
          schedulePoll(scanId, availability);
        }
      }, POLL_INTERVAL_MS);
    },
    [formatError, onScanFinished, stopPolling],
  );

  const onStartScan = useCallback(async () => {
    if (!account) return;
    setError(null);
    const availability =
      phase.kind === "detect_result" || phase.kind === "terminal"
        ? phase.availability
        : null;
    if (!availability) return;
    setPhase({ kind: "starting", availability });
    try {
      const initial = await ipc.scannerRunScan(account.aws_account_id);
      if (isTerminalScanStatus(initial.status)) {
        setPhase({ kind: "terminal", availability, record: initial });
        await onScanFinished();
      } else {
        setPhase({ kind: "running", availability, record: initial });
        schedulePoll(initial.scan_id, availability);
      }
    } catch (err) {
      setError(formatError(err));
      setPhase({ kind: "detect_result", availability });
    }
  }, [account, phase, formatError, onScanFinished, schedulePoll]);

  const onCancel = useCallback(async () => {
    if (phase.kind !== "running") return;
    setCanceling(true);
    setError(null);
    try {
      const next = await ipc.scannerCancelScan(phase.record.scan_id);
      stopPolling();
      setPhase({
        kind: "terminal",
        availability: phase.availability,
        record: next,
      });
      await onScanFinished();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setCanceling(false);
    }
  }, [phase, formatError, onScanFinished, stopPolling]);

  if (!account) return null;

  return (
    <Modal
      open={true}
      onClose={onClose}
      title={t("scanner.scan.title")}
      footer={renderFooter({
        phase,
        canceling,
        t,
        onClose,
        onStartScan,
        onCancel,
      })}
    >
      <div className="flex flex-col gap-4">
        <p className="text-small text-saw-grey-600">
          {t("scanner.scan.subtitle").replace("{label}", account.label)}
        </p>

        <AccountSummary account={account} />

        <DetectionSection phase={phase} />

        {phase.kind === "running" || phase.kind === "terminal" ? (
          <ScanRecordSection
            record={phase.record}
            includeFinishedAt={phase.kind === "terminal"}
          />
        ) : null}

        {error ? (
          <p
            role="alert"
            className="rounded-card border border-saw-grey-200 bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
            data-testid="scanner-error"
          >
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}

function renderFooter({
  phase,
  canceling,
  t,
  onClose,
  onStartScan,
  onCancel,
}: {
  phase: Phase;
  canceling: boolean;
  t: (k: string) => string;
  onClose: () => void;
  onStartScan: () => void;
  onCancel: () => void;
}) {
  switch (phase.kind) {
    case "detecting":
      return (
        <Button variant="ghost" onClick={onClose} data-testid="scanner-cancel">
          {t("common.cancel")}
        </Button>
      );
    case "detect_result": {
      const canScan = phase.availability.status === "available";
      return (
        <>
          <Button variant="ghost" onClick={onClose} data-testid="scanner-cancel">
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onStartScan}
            disabled={!canScan}
            data-testid="scanner-start"
          >
            {t("scanner.scan.start_cta")}
          </Button>
        </>
      );
    }
    case "starting":
      return (
        <>
          <Button variant="ghost" onClick={onClose} disabled data-testid="scanner-cancel">
            {t("common.cancel")}
          </Button>
          <Button variant="primary" disabled data-testid="scanner-start">
            {t("scanner.scan.starting")}
          </Button>
        </>
      );
    case "running":
      return (
        <>
          <Button
            variant="ghost"
            onClick={onClose}
            data-testid="scanner-close"
          >
            {t("scanner.scan.run_in_background")}
          </Button>
          <Button
            variant="danger"
            onClick={onCancel}
            disabled={canceling}
            data-testid="scanner-cancel-running"
          >
            {canceling
              ? t("scanner.scan.canceling")
              : t("scanner.scan.cancel_cta")}
          </Button>
        </>
      );
    case "terminal":
      return (
        <>
          <Button
            variant="primary"
            onClick={onClose}
            data-testid="scanner-close"
          >
            {t("common.close")}
          </Button>
          {phase.availability.status === "available" ? (
            <Button
              variant="secondary"
              onClick={onStartScan}
              data-testid="scanner-rerun"
            >
              {t("scanner.scan.rerun_cta")}
            </Button>
          ) : null}
        </>
      );
  }
}

function AccountSummary({ account }: { account: Account }) {
  const t = useT();
  return (
    <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 rounded-card bg-saw-grey-50 px-4 py-3 text-small">
      <dt className="text-saw-grey-500">{t("accounts.row.profile")}</dt>
      <dd className="font-mono">{account.profile_name}</dd>
      <dt className="text-saw-grey-500">{t("accounts.row.account_id")}</dt>
      <dd className="font-mono">{account.aws_account_id}</dd>
      <dt className="text-saw-grey-500">{t("accounts.row.role_status")}</dt>
      <dd>
        {account.role_provisioned
          ? t("accounts.row.role_provisioned")
          : t("accounts.row.role_not_provisioned")}
      </dd>
    </dl>
  );
}

function DetectionSection({ phase }: { phase: Phase }) {
  const t = useT();
  let availability: ScoutSuiteAvailability | null = null;
  if (
    phase.kind === "detect_result" ||
    phase.kind === "starting" ||
    phase.kind === "running" ||
    phase.kind === "terminal"
  ) {
    availability = phase.availability;
  }

  if (phase.kind === "detecting" || availability === null) {
    return (
      <p
        className="text-small text-saw-grey-600"
        data-testid="scanner-detect-state"
      >
        {t("scanner.detect.checking")}
      </p>
    );
  }

  if (availability.status === "available") {
    const shortSha = availability.sha256.slice(0, 12);
    return (
      <p
        className="flex items-center gap-2 text-small text-saw-grey-700"
        data-testid="scanner-detect-state"
      >
        <Badge tone="success">
          {t("scanner.detect.available").replace("{sha}", shortSha)}
        </Badge>
      </p>
    );
  }

  if (availability.status === "missing") {
    return (
      <div
        role="alert"
        className="rounded-card border border-saw-grey-200 bg-saw-grey-50 px-3 py-2"
        data-testid="scanner-detect-state"
      >
        <p className="text-small font-medium text-saw-grey-800">
          {t("scanner.detect.missing.title")}
        </p>
        <p className="mt-1 text-small text-saw-grey-700">
          {t("scanner.detect.missing.body")}
        </p>
      </div>
    );
  }

  return (
    <div
      role="alert"
      className="rounded-card border border-saw-grey-200 bg-saw-grey-100 px-3 py-2"
      data-testid="scanner-detect-state"
    >
      <p className="text-small font-medium text-saw-red">
        {t("scanner.detect.integrity.title")}
      </p>
      <p className="mt-1 text-small text-saw-grey-800">
        {t("scanner.detect.integrity.body")}
      </p>
    </div>
  );
}

function ScanRecordSection({
  record,
  includeFinishedAt,
}: {
  record: ScanRecord;
  includeFinishedAt: boolean;
}) {
  const t = useT();
  return (
    <div className="flex flex-col gap-3" data-testid="scanner-progress">
      <h3 className="text-small font-semibold text-saw-grey-800">
        {t("scanner.scan.progress_title")}
      </h3>

      <div className="flex items-center gap-2">
        <StatusBadge status={record.status} />
        {record.truncated ? (
          <Badge tone="neutral" data-testid="scanner-truncated">
            {t("scanner.status.truncated")}
          </Badge>
        ) : null}
      </div>

      <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-small">
        <dt className="text-saw-grey-500">
          {t("scanner.scan.session_label")}
        </dt>
        <dd
          className="font-mono text-saw-grey-700"
          data-testid="scanner-session-name"
        >
          {record.role_session_name}
        </dd>
        <dt className="text-saw-grey-500">{t("scanner.scan.started_at")}</dt>
        <dd>{formatTs(record.started_at)}</dd>
        {includeFinishedAt && record.finished_at ? (
          <>
            <dt className="text-saw-grey-500">
              {t("scanner.scan.finished_at")}
            </dt>
            <dd>{formatTs(record.finished_at)}</dd>
          </>
        ) : null}
        {record.warning_code ? (
          <>
            <dt className="text-saw-grey-500">
              {t("scanner.scan.warning_label")}
            </dt>
            <dd data-testid="scanner-warning-code">
              {t(`scanner.warning.${record.warning_code}`)}
            </dd>
          </>
        ) : null}
        {record.failure_code ? (
          <>
            <dt className="text-saw-grey-500">
              {t("scanner.scan.failure_label")}
            </dt>
            <dd className="text-saw-red" data-testid="scanner-failure-code">
              {t(`scanner.failure.${record.failure_code}`)}
            </dd>
          </>
        ) : null}
      </dl>

      {record.status === "complete" ||
      record.status === "complete_with_warnings" ? (
        <p
          className="rounded-card bg-saw-grey-50 px-3 py-2 text-small text-saw-grey-800"
          data-testid="scanner-handoff"
        >
          {t("scanner.scan.handoff")}
        </p>
      ) : null}
    </div>
  );
}

function StatusBadge({ status }: { status: ScanStatus }) {
  const t = useT();
  const tone: "success" | "info" | "danger" | "neutral" = (() => {
    switch (status) {
      case "complete":
        return "success";
      case "complete_with_warnings":
        return "info";
      case "failed":
      case "canceled":
        return "danger";
      default:
        return "neutral";
    }
  })();
  return (
    <Badge tone={tone} data-testid={`scanner-status-${status}`}>
      {t(`scanner.status.${status}`)}
    </Badge>
  );
}

function formatTs(ts: string): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString();
}
