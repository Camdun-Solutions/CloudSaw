// ScheduledScans — Settings sub-panel. Lists configured accounts, lets the
// user pick a cadence per account, and surfaces the precomputed next-run
// time. The configuration is plain non-secret data — no AWS calls happen
// in this surface (CLAUDE.md §4.3; Contract 10 §Constraints).
//
// Architecture: every action goes through `ipc.scheduler*` — no `invoke()`
// calls live here and no IPC payload contains credential material. The
// component re-reads schedules after every mutation so it stays in sync
// with the background runner without bespoke event plumbing.

import { useCallback, useEffect, useMemo, useState } from "react";

import { BackBreadcrumb, Badge, Button, EmptyState, Select, Switch } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  maskAccountId,
  type Account,
  type LastRunOutcome,
  type Schedule,
  type ScheduleCadence,
  type SetScheduleInput,
} from "@/lib/ipc";

type CadenceChoice = "daily" | "weekly" | "monthly" | "interval";

type DraftState = {
  cadence: CadenceChoice;
  /** Used for daily/weekly/monthly. */
  hour: number;
  minute: number;
  /** 0..6, used for weekly. */
  day_of_week: number;
  /** 1..28, used for monthly. */
  day_of_month: number;
  /** Used for interval cadences. */
  interval_minutes: number;
  enabled: boolean;
};

function defaultDraft(): DraftState {
  return {
    cadence: "weekly",
    hour: 9,
    minute: 0,
    day_of_week: 1,
    day_of_month: 1,
    interval_minutes: 60,
    enabled: true,
  };
}

function draftFromSchedule(s: Schedule): DraftState {
  const draft = defaultDraft();
  const tod = s.time_of_day_minutes ?? 0;
  draft.hour = Math.floor(tod / 60);
  draft.minute = tod % 60;
  draft.enabled = s.enabled;
  switch (s.cadence.kind) {
    case "daily":
      draft.cadence = "daily";
      break;
    case "weekly":
      draft.cadence = "weekly";
      draft.day_of_week = s.cadence.day_of_week;
      break;
    case "monthly":
      draft.cadence = "monthly";
      draft.day_of_month = s.cadence.day_of_month;
      break;
    case "interval":
      draft.cadence = "interval";
      draft.interval_minutes = s.cadence.minutes;
      break;
  }
  return draft;
}

function draftToCadence(draft: DraftState): ScheduleCadence {
  switch (draft.cadence) {
    case "daily":
      return { kind: "daily" };
    case "weekly":
      return { kind: "weekly", day_of_week: draft.day_of_week };
    case "monthly":
      return { kind: "monthly", day_of_month: draft.day_of_month };
    case "interval":
      return { kind: "interval", minutes: draft.interval_minutes };
  }
}

function draftToTimeOfDay(draft: DraftState): number | null {
  if (draft.cadence === "interval") return null;
  return draft.hour * 60 + draft.minute;
}

function formatLocalTimestamp(iso: string | null): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

function lastRunOutcomeKey(outcome: LastRunOutcome | null): string {
  if (!outcome) return "schedules.last_run.none";
  return `schedules.last_run.${outcome}`;
}

type Props = { onBack: () => void };

export default function ScheduledScans({ onBack }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(
    null,
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [acctList, schedList] = await Promise.all([
        ipc.accountsList(),
        ipc.schedulerListSchedules(),
      ]);
      setAccounts(acctList);
      setSchedules(schedList);
      if (acctList.length > 0 && !selectedAccountId) {
        setSelectedAccountId(acctList[0].aws_account_id);
      }
    } catch (err) {
      setLoadError(formatError(err));
    } finally {
      setLoading(false);
    }
  }, [formatError, selectedAccountId]);

  useEffect(() => {
    void refresh();
    // Re-fetch every 30s so next-run times tick down without an explicit
    // event. The runner itself polls on a similar cadence; aligning the UI
    // gives the user a confidence cue.
    const handle = window.setInterval(() => {
      void refresh();
    }, 30_000);
    return () => window.clearInterval(handle);
    // We deliberately depend only on `refresh` — the callback is recreated
    // when its dependencies change.
  }, [refresh]);

  const selectedAccount = useMemo(
    () => accounts.find((a) => a.aws_account_id === selectedAccountId) ?? null,
    [accounts, selectedAccountId],
  );

  const selectedSchedule = useMemo(
    () =>
      schedules.find((s) => s.aws_account_id === selectedAccountId) ?? null,
    [schedules, selectedAccountId],
  );

  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black px-8 py-10">
      <header className="mb-6">
        <BackBreadcrumb
          destination={t("nav.settings")}
          onBack={onBack}
          data-testid="schedules-back"
        />
        <h1 className="mt-2 text-h1 font-semibold text-saw-grey-900 dark:text-saw-beige">
          {t("schedules.title")}
        </h1>
        <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("schedules.subtitle")}
        </p>
      </header>

      <section
        className="grid max-w-5xl gap-6 lg:grid-cols-[260px_1fr]"
        data-testid="schedules-section"
      >
        <aside className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-3">
          <p className="px-2 pb-2 text-small font-medium text-saw-grey-600 dark:text-saw-grey-400">
            {t("schedules.accounts_label")}
          </p>
          {loading && accounts.length === 0 ? (
            <p className="px-2 py-3 text-small text-saw-grey-500 dark:text-saw-grey-400">
              {t("common.loading")}
            </p>
          ) : accounts.length === 0 ? (
            <EmptyState
              title={t("schedules.no_accounts.title")}
              body={t("schedules.no_accounts.body")}
            />
          ) : (
            <ul className="flex flex-col gap-1">
              {accounts.map((account) => {
                const schedule = schedules.find(
                  (s) => s.aws_account_id === account.aws_account_id,
                );
                const isSelected =
                  selectedAccountId === account.aws_account_id;
                return (
                  <li key={account.aws_account_id}>
                    <button
                      type="button"
                      onClick={() =>
                        setSelectedAccountId(account.aws_account_id)
                      }
                      data-testid={`schedules-pick-${account.aws_account_id}`}
                      className={[
                        "flex w-full flex-col items-start gap-1 rounded-card px-3 py-2 text-left",
                        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange",
                        isSelected
                          ? "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-900 dark:text-saw-beige"
                          : "text-saw-grey-700 dark:text-saw-grey-300 hover:bg-saw-grey-50 dark:hover:bg-saw-grey-800",
                      ].join(" ")}
                    >
                      <span className="text-body font-medium">
                        {account.label}
                      </span>
                      <span className="text-small text-saw-grey-500 dark:text-saw-grey-400">
                        {maskAccountId(account.aws_account_id)}
                      </span>
                      {schedule ? (
                        <Badge tone={schedule.enabled ? "success" : "neutral"}>
                          {schedule.enabled
                            ? t("schedules.row.enabled")
                            : t("schedules.row.disabled")}
                        </Badge>
                      ) : (
                        <Badge tone="neutral">
                          {t("schedules.row.no_schedule")}
                        </Badge>
                      )}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </aside>

        <div>
          {loadError ? (
            <p
              role="alert"
              data-testid="schedules-load-error"
              className="mb-4 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            >
              {loadError}
            </p>
          ) : null}
          {selectedAccount ? (
            <SchedulePanel
              account={selectedAccount}
              schedule={selectedSchedule}
              onChanged={refresh}
            />
          ) : (
            <p className="text-body text-saw-grey-600 dark:text-saw-grey-400">
              {t("schedules.pick_an_account")}
            </p>
          )}
        </div>
      </section>
    </main>
  );
}

function SchedulePanel({
  account,
  schedule,
  onChanged,
}: {
  account: Account;
  schedule: Schedule | null;
  onChanged: () => Promise<void>;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [draft, setDraft] = useState<DraftState>(() =>
    schedule ? draftFromSchedule(schedule) : defaultDraft(),
  );
  const [saving, setSaving] = useState(false);
  const [savedFlash, setSavedFlash] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset the draft when the user selects a different account.
  useEffect(() => {
    setDraft(schedule ? draftFromSchedule(schedule) : defaultDraft());
    setError(null);
    setSavedFlash(false);
  }, [account.aws_account_id, schedule]);

  const minuteOptions = useMemo(
    () =>
      [0, 15, 30, 45].map((m) => ({
        value: String(m) as `${number}`,
        label: m.toString().padStart(2, "0"),
      })),
    [],
  );

  const hourOptions = useMemo(
    () =>
      Array.from({ length: 24 }, (_, i) => ({
        value: String(i) as `${number}`,
        label: i.toString().padStart(2, "0"),
      })),
    [],
  );

  const cadenceOptions: { value: CadenceChoice; label: string }[] = [
    { value: "daily", label: t("schedules.cadence.daily") },
    { value: "weekly", label: t("schedules.cadence.weekly") },
    { value: "monthly", label: t("schedules.cadence.monthly") },
    { value: "interval", label: t("schedules.cadence.interval") },
  ];

  const dayOfWeekOptions: { value: `${number}`; label: string }[] = [
    { value: "0", label: t("schedules.dow.sunday") },
    { value: "1", label: t("schedules.dow.monday") },
    { value: "2", label: t("schedules.dow.tuesday") },
    { value: "3", label: t("schedules.dow.wednesday") },
    { value: "4", label: t("schedules.dow.thursday") },
    { value: "5", label: t("schedules.dow.friday") },
    { value: "6", label: t("schedules.dow.saturday") },
  ];

  const dayOfMonthOptions = useMemo(
    () =>
      Array.from({ length: 28 }, (_, i) => {
        const v = (i + 1).toString() as `${number}`;
        return { value: v, label: v };
      }),
    [],
  );

  const intervalOptions: { value: `${number}`; label: string }[] = [
    { value: "15", label: t("schedules.interval.15m") },
    { value: "60", label: t("schedules.interval.1h") },
    { value: "240", label: t("schedules.interval.4h") },
    { value: "720", label: t("schedules.interval.12h") },
    { value: "1440", label: t("schedules.interval.1d") },
  ];

  async function onSave() {
    setSaving(true);
    setSavedFlash(false);
    setError(null);
    try {
      const payload: SetScheduleInput = {
        aws_account_id: account.aws_account_id,
        cadence: draftToCadence(draft),
        time_of_day_minutes: draftToTimeOfDay(draft),
        enabled: draft.enabled,
      };
      await ipc.schedulerSetSchedule(payload);
      await onChanged();
      setSavedFlash(true);
      window.setTimeout(() => setSavedFlash(false), 2000);
    } catch (err) {
      setError(formatError(err));
    } finally {
      setSaving(false);
    }
  }

  async function onClear() {
    if (!schedule) return;
    setSaving(true);
    setError(null);
    try {
      await ipc.schedulerClearSchedule(account.aws_account_id);
      await onChanged();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setSaving(false);
    }
  }

  const roleNotProvisioned = !account.role_provisioned;

  return (
    <section
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-6"
      data-testid="schedules-panel"
    >
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
            {account.label}
          </h2>
          <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
            {t("schedules.panel.subtitle")}
          </p>
        </div>
        <div className="flex flex-col gap-1 text-right">
          <p className="text-small text-saw-grey-500 dark:text-saw-grey-400">
            {t("schedules.panel.next_run")}
          </p>
          <p
            className="text-body font-medium text-saw-grey-900 dark:text-saw-beige"
            data-testid="schedules-next-run"
          >
            {schedule?.enabled
              ? formatLocalTimestamp(schedule.next_run_at)
              : "—"}
          </p>
        </div>
      </div>

      {roleNotProvisioned ? (
        <p
          role="status"
          data-testid="schedules-role-warning"
          className="mt-4 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
        >
          {t("schedules.panel.role_warning")}
        </p>
      ) : null}

      <div className="mt-6 grid gap-4 sm:grid-cols-2">
        <Select<CadenceChoice>
          label={t("schedules.field.cadence")}
          value={draft.cadence}
          options={cadenceOptions}
          onChange={(next) => setDraft((d) => ({ ...d, cadence: next }))}
          data-testid="schedules-cadence"
        />
        {draft.cadence === "weekly" ? (
          <Select<`${number}`>
            label={t("schedules.field.day_of_week")}
            value={String(draft.day_of_week) as `${number}`}
            options={dayOfWeekOptions}
            onChange={(next) =>
              setDraft((d) => ({ ...d, day_of_week: Number(next) }))
            }
            data-testid="schedules-dow"
          />
        ) : null}
        {draft.cadence === "monthly" ? (
          <Select<`${number}`>
            label={t("schedules.field.day_of_month")}
            value={String(draft.day_of_month) as `${number}`}
            options={dayOfMonthOptions}
            onChange={(next) =>
              setDraft((d) => ({ ...d, day_of_month: Number(next) }))
            }
            data-testid="schedules-dom"
          />
        ) : null}
        {draft.cadence === "interval" ? (
          <Select<`${number}`>
            label={t("schedules.field.interval")}
            value={String(draft.interval_minutes) as `${number}`}
            options={intervalOptions}
            onChange={(next) =>
              setDraft((d) => ({ ...d, interval_minutes: Number(next) }))
            }
            data-testid="schedules-interval"
          />
        ) : null}
        {draft.cadence !== "interval" ? (
          <div className="flex gap-3">
            <Select<`${number}`>
              label={t("schedules.field.hour")}
              value={String(draft.hour) as `${number}`}
              options={hourOptions}
              onChange={(next) =>
                setDraft((d) => ({ ...d, hour: Number(next) }))
              }
              data-testid="schedules-hour"
            />
            <Select<`${number}`>
              label={t("schedules.field.minute")}
              value={String(draft.minute) as `${number}`}
              options={minuteOptions}
              onChange={(next) =>
                setDraft((d) => ({ ...d, minute: Number(next) }))
              }
              data-testid="schedules-minute"
            />
          </div>
        ) : null}
      </div>

      <div className="mt-6">
        <Switch
          label={t("schedules.field.enabled")}
          description={t("schedules.field.enabled_hint")}
          checked={draft.enabled}
          onChange={(next) => setDraft((d) => ({ ...d, enabled: next }))}
        />
      </div>

      {error ? (
        <p
          role="alert"
          data-testid="schedules-save-error"
          className="mt-4 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
        >
          {error}
        </p>
      ) : null}
      {savedFlash ? (
        <p
          role="status"
          data-testid="schedules-saved"
          className="mt-4 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
        >
          {t("schedules.panel.saved")}
        </p>
      ) : null}

      <div className="mt-6 flex flex-wrap gap-3">
        <Button
          variant="primary"
          onClick={onSave}
          disabled={saving}
          data-testid="schedules-save"
        >
          {saving ? t("common.loading") : t("common.save")}
        </Button>
        {schedule ? (
          <Button
            variant="ghost"
            onClick={onClear}
            disabled={saving}
            data-testid="schedules-clear"
          >
            {t("schedules.panel.clear")}
          </Button>
        ) : null}
      </div>

      {schedule ? (
        <div className="mt-8 border-t border-saw-grey-200 dark:border-saw-grey-700 pt-6">
          <h3 className="text-small font-semibold uppercase tracking-wide text-saw-grey-600 dark:text-saw-grey-400">
            {t("schedules.panel.last_run_title")}
          </h3>
          <dl className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div>
              <dt className="text-small text-saw-grey-500 dark:text-saw-grey-400">
                {t("schedules.panel.last_run_at")}
              </dt>
              <dd
                className="text-body text-saw-grey-900 dark:text-saw-beige"
                data-testid="schedules-last-run-at"
              >
                {formatLocalTimestamp(schedule.last_run_at)}
              </dd>
            </div>
            <div>
              <dt className="text-small text-saw-grey-500 dark:text-saw-grey-400">
                {t("schedules.panel.last_run_outcome")}
              </dt>
              <dd
                className="text-body text-saw-grey-900 dark:text-saw-beige"
                data-testid="schedules-last-run-outcome"
              >
                {t(lastRunOutcomeKey(schedule.last_run_outcome))}
              </dd>
            </div>
          </dl>
        </div>
      ) : null}
    </section>
  );
}
