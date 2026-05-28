// Scheduled scans — Settings → Schedules embedded panel.
//
// PR #67: the standalone /schedules route is gone. This component
// now renders inline in `Settings.tsx`'s Schedules section. The
// "configure cadence" form moved into a Modal that opens when the
// user clicks "Add schedule" (or "Edit" on an existing schedule
// row). Configured schedules render as a list below the section
// header.
//
// Architecture: every action goes through `ipc.scheduler*` — no
// `invoke()` calls live here and no IPC payload contains credential
// material. The component re-reads schedules after every mutation
// so it stays in sync with the background runner without bespoke
// event plumbing.

import { useCallback, useEffect, useMemo, useState } from "react";

import { Badge, Button, EmptyState, Modal, Select, Switch } from "@/components";
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

function describeCadence(s: Schedule, t: (k: string) => string): string {
  switch (s.cadence.kind) {
    case "daily":
      return t("schedules.cadence.daily");
    case "weekly":
      return `${t("schedules.cadence.weekly")} (${t(`schedules.dow.${["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"][s.cadence.day_of_week]}`)})`;
    case "monthly":
      return `${t("schedules.cadence.monthly")} (${s.cadence.day_of_month})`;
    case "interval":
      return `${t("schedules.cadence.interval")} (${s.cadence.minutes}m)`;
  }
}

/** Modal phase. "pick" lets the user choose an account to schedule;
 *  "form" shows the cadence editor for a specific account. The
 *  parent transitions pick→form when the user picks an account. */
type ModalPhase =
  | { kind: "pick" }
  | { kind: "form"; account: Account };

export default function ScheduledScans() {
  const t = useT();
  const formatError = useIpcError();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [modalPhase, setModalPhase] = useState<ModalPhase | null>(null);

  const refresh = useCallback(async () => {
    setLoadError(null);
    try {
      const [acctList, schedList] = await Promise.all([
        ipc.accountsList(),
        ipc.schedulerListSchedules(),
      ]);
      setAccounts(acctList);
      setSchedules(schedList);
    } catch (err) {
      setLoadError(formatError(err));
    }
  }, [formatError]);

  useEffect(() => {
    void refresh();
    // Re-fetch every 30s so next-run times tick down without an
    // explicit event. The runner itself polls on a similar cadence;
    // aligning the UI gives the user a confidence cue.
    const handle = window.setInterval(() => {
      void refresh();
    }, 30_000);
    return () => window.clearInterval(handle);
  }, [refresh]);

  const accountById = useCallback(
    (id: string) => accounts.find((a) => a.aws_account_id === id) ?? null,
    [accounts],
  );

  const scheduleForAccount = useCallback(
    (id: string) => schedules.find((s) => s.aws_account_id === id) ?? null,
    [schedules],
  );

  // Accounts that don't already have a schedule — the "pick" phase
  // restricts to these so the user doesn't try to add a duplicate.
  const unscheduledAccounts = useMemo(
    () =>
      accounts.filter(
        (a) => !schedules.some((s) => s.aws_account_id === a.aws_account_id),
      ),
    [accounts, schedules],
  );

  return (
    <section
      className="mt-6 max-w-3xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-schedules"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
            {t("schedules.section_title")}
          </h2>
          <p className="mt-1 max-w-2xl text-small text-saw-grey-600 dark:text-saw-grey-400">
            {t("schedules.section_subtitle")}
          </p>
        </div>
        <Button
          variant="primary"
          onClick={() => setModalPhase({ kind: "pick" })}
          disabled={accounts.length === 0}
          data-testid="schedules-add"
        >
          {t("schedules.add_cta")}
        </Button>
      </div>

      {loadError ? (
        <p
          role="alert"
          data-testid="schedules-load-error"
          className="mt-4 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
        >
          {loadError}
        </p>
      ) : null}

      <div className="mt-4" data-testid="schedules-list">
        {accounts.length === 0 ? (
          <EmptyState
            title={t("schedules.no_accounts.title")}
            body={t("schedules.no_accounts.body")}
          />
        ) : schedules.length === 0 ? (
          <p
            className="rounded-card border border-dashed border-saw-grey-200 dark:border-saw-grey-700 px-4 py-6 text-center text-small text-saw-grey-600 dark:text-saw-grey-400"
            data-testid="schedules-empty"
          >
            {t("schedules.empty")}
          </p>
        ) : (
          <ul
            className="divide-y divide-saw-grey-200 dark:divide-saw-grey-700 rounded-card border border-saw-grey-200 dark:border-saw-grey-700"
            data-testid="schedules-rows"
          >
            {schedules.map((s) => {
              const account = accountById(s.aws_account_id);
              return (
                <li
                  key={s.aws_account_id}
                  className="flex flex-wrap items-center gap-3 px-4 py-3"
                  data-testid={`schedule-row-${s.aws_account_id}`}
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-body font-medium text-saw-grey-900 dark:text-saw-beige">
                      {account?.label ?? maskAccountId(s.aws_account_id)}
                    </p>
                    <p className="text-small text-saw-grey-500 dark:text-saw-grey-400">
                      {describeCadence(s, t)}
                    </p>
                    <p className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
                      {t("schedules.row.next_run_label")}:{" "}
                      {s.enabled ? formatLocalTimestamp(s.next_run_at) : "—"}
                    </p>
                  </div>
                  <Badge tone={s.enabled ? "success" : "neutral"}>
                    {s.enabled
                      ? t("schedules.row.enabled")
                      : t("schedules.row.disabled")}
                  </Badge>
                  {account ? (
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() =>
                        setModalPhase({ kind: "form", account })
                      }
                      data-testid={`schedule-edit-${s.aws_account_id}`}
                    >
                      {t("schedules.row.edit_cta")}
                    </Button>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {/* PR #67: Modal for Add / Edit. "pick" phase lets the user
          select an account; once picked, the modal transitions to
          "form" with the SchedulePanel. Edit jumps straight to
          "form" with the row's account pre-set. */}
      {modalPhase ? (
        <Modal
          open={true}
          onClose={() => setModalPhase(null)}
          title={
            modalPhase.kind === "pick"
              ? t("schedules.modal.pick_title")
              : t("schedules.modal.form_title").replace(
                  "{label}",
                  modalPhase.account.label,
                )
          }
          size="lg"
        >
          {modalPhase.kind === "pick" ? (
            <PickAccount
              accounts={unscheduledAccounts}
              onPick={(account) => setModalPhase({ kind: "form", account })}
              onCancel={() => setModalPhase(null)}
            />
          ) : (
            <SchedulePanel
              account={modalPhase.account}
              schedule={scheduleForAccount(modalPhase.account.aws_account_id)}
              onChanged={async () => {
                await refresh();
                setModalPhase(null);
              }}
            />
          )}
        </Modal>
      ) : null}
    </section>
  );
}

/** Account picker rendered inside the Add-schedule modal. Lists
 *  accounts that don't already have a schedule. The user picks one
 *  to transition into the cadence form. */
function PickAccount({
  accounts,
  onPick,
  onCancel,
}: {
  accounts: Account[];
  onPick: (account: Account) => void;
  onCancel: () => void;
}) {
  const t = useT();
  if (accounts.length === 0) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-body text-saw-grey-600 dark:text-saw-grey-400">
          {t("schedules.modal.pick_all_taken")}
        </p>
        <div className="flex justify-end">
          <Button variant="ghost" onClick={onCancel}>
            {t("common.close")}
          </Button>
        </div>
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-2" data-testid="schedules-pick-list">
      {accounts.map((a) => (
        <li key={a.aws_account_id}>
          <button
            type="button"
            onClick={() => onPick(a)}
            data-testid={`schedules-pick-${a.aws_account_id}`}
            className="flex w-full flex-col items-start gap-1 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-4 py-3 text-left transition hover:border-saw-red hover:bg-saw-grey-50 dark:hover:bg-saw-grey-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-red"
          >
            <span className="font-medium text-saw-grey-900 dark:text-saw-beige">
              {a.label}
            </span>
            <span className="font-mono text-xs text-saw-grey-500 dark:text-saw-grey-400">
              {maskAccountId(a.aws_account_id)} · {a.profile_name}
            </span>
          </button>
        </li>
      ))}
    </ul>
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

  // Reset the draft when the modal's target account changes.
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
    <div className="flex flex-col gap-4" data-testid="schedules-panel">
      <p className="text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("schedules.panel.subtitle")}
      </p>
      {schedule ? (
        <p
          className="text-small text-saw-grey-500 dark:text-saw-grey-400"
          data-testid="schedules-next-run"
        >
          {t("schedules.panel.next_run")}:{" "}
          {schedule.enabled ? formatLocalTimestamp(schedule.next_run_at) : "—"}
        </p>
      ) : null}

      {roleNotProvisioned ? (
        <p
          role="status"
          data-testid="schedules-role-warning"
          className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
        >
          {t("schedules.panel.role_warning")}
        </p>
      ) : null}

      <div className="grid gap-4 sm:grid-cols-2">
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

      <Switch
        label={t("schedules.field.enabled")}
        description={t("schedules.field.enabled_hint")}
        checked={draft.enabled}
        onChange={(next) => setDraft((d) => ({ ...d, enabled: next }))}
      />

      {error ? (
        <p
          role="alert"
          data-testid="schedules-save-error"
          className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
        >
          {error}
        </p>
      ) : null}
      {savedFlash ? (
        <p
          role="status"
          data-testid="schedules-saved"
          className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
        >
          {t("schedules.panel.saved")}
        </p>
      ) : null}

      <div className="flex flex-wrap gap-3">
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
        <div className="border-t border-saw-grey-200 dark:border-saw-grey-700 pt-4">
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
    </div>
  );
}
