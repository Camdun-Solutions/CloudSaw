// Settings — lock period, biometric toggle, change-password.
//
// This is the only post-unlock screen Contract 02 owns. Later contracts will
// build out the full settings surface; for now Settings is a single panel
// dedicated to app-lock configuration, reachable from the main header.

import { useCallback, useEffect, useState } from "react";

import { Button, Logo, Modal, PasswordField, Select, Switch, TagInput } from "@/components";
import { useAppearance } from "@/hooks/useAppearance";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import type { Appearance } from "@/lib/appearance";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import Accounts from "@/routes/Accounts";
import ActivityLog from "@/routes/ActivityLog";
import ScheduledScans from "@/routes/ScheduledScans";
import { CustomReportModal } from "@/components";
import {
  isScanNotificationsEnabled,
  setScanNotificationsEnabled,
} from "@/lib/scanNotifications";

import {
  ipc,
  JOB_ROLE_MAX_LEN,
  KNOWN_COMPLIANCE_FRAMEWORKS,
  type AiProvider,
  type AiSettings as AiSettingsT,
  type BusinessContext,
  type EnvironmentType,
  type GithubSettings,
  type LockPeriod,
  type LockSettings,
  type PanicWipeResult,
  type ProviderRecord,
  type ReportSettings as ReportSettingsT,
  type RetentionPeriod,
  type RetentionSettings,
  type RiskTolerance,
  type TeamSize,
} from "@/lib/ipc";
import { useLock } from "@/stores/lock";

/** The 11 sections of the Settings page, in left-nav order.
 *  PR #48 (this PR) introduces the left-menu layout — only one
 *  section renders in the right panel at a time. Section IDs are
 *  stable strings so a future PR can wire deep-linking via URL
 *  hash / route sub-path. */
export type SettingsSection =
  | "app_lock"
  | "accounts"
  | "appearance"
  | "notifications"
  | "schedules"
  | "activity_log"
  | "report"
  | "retention"
  | "updates"
  | "github"
  | "ai"
  | "panic";

const SECTION_ORDER: SettingsSection[] = [
  "app_lock",
  "accounts",
  // PR #57: Light/Dark theme switcher. Placed near the top of the
  // nav so a user looking to flip the theme isn't hunting past
  // ten other sections to find it.
  "appearance",
  "notifications",
  "schedules",
  "activity_log",
  // PR #67: "onboarding" removed — re-onboarding now happens only
  // via the new Reset Application flow under the renamed "Reset"
  // (formerly "Panic wipe") section.
  "report",
  "retention",
  "updates",
  "github",
  "ai",
  "panic",
];

type PeriodChoice = "immediate" | "1d" | "7d" | "30d" | "never";

const PERIOD_TO_CHOICE = (p: LockPeriod): PeriodChoice => {
  if (p.kind === "immediate") return "immediate";
  if (p.kind === "never") return "never";
  switch (p.seconds) {
    case 86400:
      return "1d";
    case 604800:
      return "7d";
    case 2592000:
      return "30d";
    default:
      return "7d";
  }
};

const CHOICE_TO_PERIOD = (c: PeriodChoice): LockPeriod => {
  switch (c) {
    case "immediate":
      return { kind: "immediate" };
    case "1d":
      return { kind: "after", seconds: 86400 };
    case "7d":
      return { kind: "after", seconds: 604800 };
    case "30d":
      return { kind: "after", seconds: 2592000 };
    case "never":
      return { kind: "never" };
  }
};

type Props = {
  /** Optional deep-link target. When set, Settings opens with the
   *  given left-nav section pre-selected instead of the default
   *  `app_lock`. PR #63 (scan-modal Settings CTAs) uses this to land
   *  the user on `accounts` from the scan-modal's no-accounts /
   *  role-not-provisioned CTAs. */
  initialSection?: SettingsSection;
  /** Bumped by the parent each time it wants the deep-link to
   *  re-fire — without it, repeated CTAs to the same `initialSection`
   *  wouldn't trigger the effect because React sees no prop change.
   *  Parent owns the counter; Settings only reads it. */
  initialSectionNonce?: number;
};

export default function Settings({
  initialSection,
  initialSectionNonce,
}: Props) {
  const t = useT();
  const formatError = useIpcError();
  const { state, refresh } = useLock();

  const [period, setPeriod] = useState<PeriodChoice>("7d");
  const [biometric, setBiometric] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const [changeOpen, setChangeOpen] = useState(false);
  // PR #48 — left-menu pane: only one section renders at a time
  // in the right panel. Defaults to App Lock (the original
  // top-of-page section). A future PR will wire deep-linking
  // from the parent route's sub-path.
  const [activeSection, setActiveSection] =
    useState<SettingsSection>(initialSection ?? "app_lock");

  // Re-apply the deep-link target whenever the parent re-targets,
  // even if the section value is unchanged from the previous call.
  // `initialSectionNonce` is what guarantees the effect fires on a
  // repeat tap of the same section — React's dep array sees a new
  // value each time the parent calls `goToSettingsSection`.
  useEffect(() => {
    if (initialSection) {
      setActiveSection(initialSection);
    }
  }, [initialSection, initialSectionNonce]);

  // Hydrate from store on mount / when settings change underneath us.
  useEffect(() => {
    if (!state) return;
    setPeriod(PERIOD_TO_CHOICE(state.settings.lock_period));
    setBiometric(state.settings.biometric_enabled);
  }, [state]);

  const biometricSupported = state?.biometric_availability === "Available";
  const biometricUnconfigured =
    state?.biometric_availability === "Unconfigured";

  async function save() {
    setSaving(true);
    setSaveError(null);
    setSavedFlash(false);
    try {
      const next: LockSettings = {
        lock_period: CHOICE_TO_PERIOD(period),
        biometric_enabled: biometric,
      };
      await ipc.applockSetSettings(next);
      await refresh();
      setSavedFlash(true);
      window.setTimeout(() => setSavedFlash(false), 2000);
    } catch (err) {
      setSaveError(formatError(err));
    } finally {
      setSaving(false);
    }
  }

  // PR #69: `onLock` helper removed alongside the App-lock "Lock now"
  // button — the persistent TopNav already exposes a lock icon.

  if (!state) return null;

  const periodOptions: { value: PeriodChoice; label: string }[] = [
    { value: "immediate", label: t("applock.settings.period.immediate") },
    { value: "1d", label: t("applock.settings.period.1d") },
    { value: "7d", label: t("applock.settings.period.7d") },
    { value: "30d", label: t("applock.settings.period.30d") },
    { value: "never", label: t("applock.settings.period.never") },
  ];

  // Each section's nav label uses the i18n keys added in PR #48.
  // English defaults are short enough to fit a 224px-wide sidebar.
  const sectionLabels: Record<SettingsSection, string> = {
    app_lock: t("settings.nav.app_lock"),
    accounts: t("settings.nav.accounts"),
    appearance: t("settings.nav.appearance"),
    notifications: t("settings.nav.notifications"),
    schedules: t("settings.nav.schedules"),
    activity_log: t("settings.nav.activity_log"),
    report: t("settings.nav.report"),
    retention: t("settings.nav.retention"),
    updates: t("settings.nav.updates"),
    github: t("settings.nav.github"),
    ai: t("settings.nav.ai"),
    panic: t("settings.nav.panic"),
  };

  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black">
      {/* PR #77 — Settings header now mirrors the Dashboard /
          Findings / Accounts pattern: opaque `bg-saw-white
          dark:bg-saw-grey-dark` bar (was main-bg-matched, which
          made it read as a transparent section title rather than a
          true app chrome bar), Logo + text-h2 title + subtitle on
          one row. Sticky behavior + the max-w-7xl alignment with
          the body inherit from the surrounding wrapper. */}
      <header className="sticky top-0 z-20 border-b border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark">
        {/* PR #55: max-w-7xl mx-auto wraps the Settings content so
            the two-column (left-nav + right-panel) layout stays
            readable on ultra-wide displays instead of spreading the
            right panel across the whole viewport. The header carries
            the same wrapper so its title aligns with the body. */}
        <div className="mx-auto flex max-w-7xl items-center gap-3 px-8 py-5">
          <Logo size="sm" />
          <div className="flex flex-col">
            <h1 className="text-h2 font-semibold tracking-tight text-saw-grey-900 dark:text-saw-beige">
              {t("nav.settings")}
            </h1>
            <p className="text-small text-saw-grey-500 dark:text-saw-grey-400">
              {t("applock.settings.subtitle")}
            </p>
          </div>
        </div>
      </header>

      <div className="mx-auto max-w-7xl px-8 pb-10 pt-6">
      {/* PR #48 two-column layout: left nav (w-56) selects the
          active section; right panel renders that section only.
          The page's existing 10 section cards each become a
          panel — visually unchanged inside but only one mounts
          at a time, which both reduces visual noise and keeps
          per-section IPC fetch costs low (e.g. UpdatesSection
          only polls when active). */}
      <div className="flex items-start gap-6">
        <nav
          aria-label={t("settings.nav.aria_label")}
          data-testid="settings-nav"
          // PR #75: bumped sticky-offset from `top-3` (12px) to
          // `top-28` (~112px) so the inner left-nav now anchors
          // BELOW the new sticky page header instead of sliding up
          // underneath it. The header is ~90-100px tall (py-5 +
          // h1 + subtitle); 28*4=112px leaves a small gap.
          className="sticky top-28 flex w-56 shrink-0 flex-col gap-1 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-2"
        >
          {SECTION_ORDER.map((section) => {
            const isActive = activeSection === section;
            return (
              <button
                key={section}
                type="button"
                onClick={() => setActiveSection(section)}
                aria-current={isActive ? "page" : undefined}
                data-testid={`settings-nav-${section}`}
                className={
                  isActive
                    ? "rounded-card bg-saw-red px-3 py-2 text-left text-small font-medium text-saw-white"
                    : "rounded-card px-3 py-2 text-left text-small font-medium text-saw-grey-700 dark:text-saw-grey-300 transition hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800 hover:text-saw-grey-900 dark:hover:text-saw-beige"
                }
              >
                {sectionLabels[section]}
              </button>
            );
          })}
        </nav>

        {/* PR #69: right-panel cards now align with the sticky left
            nav's top edge. Sections previously carried their own
            `mt-6` which pushed the top edge ~24px below the nav.
            Spacing between stacked sections inside a single panel
            (the Reset section + Panic section, etc.) comes from
            `space-y-6` here so single-section panels render flush
            and stacked panels still breathe. */}
        <div className="min-w-0 flex-1 space-y-6" data-testid="settings-panel">

      {activeSection === "app_lock" && (
      <section
        data-testid="settings-section-app_lock"
        className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      >
        <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
          {t("applock.settings.section.app_lock")}
        </h2>
        <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("applock.disclosure")}
        </p>

        <div className="mt-6 flex flex-col gap-6">
          <Select<PeriodChoice>
            label={t("applock.settings.period.label")}
            value={period}
            options={periodOptions}
            onChange={setPeriod}
            description={
              period === "never"
                ? t("applock.settings.period.never_warning")
                : undefined
            }
            data-testid="settings-period"
          />

          <Switch
            label={t("applock.settings.biometric.label")}
            description={t("applock.settings.biometric.description")}
            checked={biometric && biometricSupported}
            onChange={setBiometric}
            disabled={!biometricSupported}
            disabledReason={
              biometricUnconfigured
                ? t("applock.settings.biometric.unconfigured")
                : t("applock.settings.biometric.unavailable")
            }
          />

          {saveError ? (
            <p
              role="alert"
              className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            >
              {saveError}
            </p>
          ) : null}
          {savedFlash ? (
            <p
              role="status"
              className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            >
              {t("applock.settings.saved")}
            </p>
          ) : null}

          <div className="flex flex-wrap gap-3">
            <Button
              variant="primary"
              onClick={save}
              disabled={saving}
              data-testid="settings-save"
            >
              {saving ? t("common.loading") : t("common.save")}
            </Button>
            <Button
              variant="secondary"
              onClick={() => setChangeOpen(true)}
              data-testid="settings-change-password"
            >
              {t("applock.settings.change_password")}
            </Button>
            {/* PR #69: "Lock now" removed from App lock settings —
                the persistent TopNav already exposes a lock icon,
                so the duplicate button was redundant. */}
          </div>
        </div>
      </section>
      )}

      {activeSection === "accounts" && (
      <section
        className="max-w-4xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
        data-testid="settings-section-accounts"
      >
        <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
          {t("accounts.title")}
        </h2>
        <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("accounts.subtitle")}
        </p>
        <div className="mt-4">
          <Accounts embedded />
        </div>
      </section>
      )}

      {activeSection === "appearance" && <AppearanceSection />}

      {activeSection === "notifications" && <NotificationsSection />}

      {/* PR #67: Schedules now renders inline — the standalone
          /schedules route is gone. The cadence form lives in a
          modal inside the embedded panel. */}
      {activeSection === "schedules" && <ScheduledScans />}

      {/* PR #67: ActivityLog now owns its own section card +
          header (so the Export button can anchor at the top-right
          of the section). The Settings wrapper for this entry is
          just a direct render. */}
      {activeSection === "activity_log" && <ActivityLog />}
      {activeSection === "report" && <ReportSection />}
      {activeSection === "retention" && <RetentionSection />}
      {activeSection === "updates" && <UpdatesSection />}
      {activeSection === "github" && <GithubSection />}
      {activeSection === "ai" && <AiSection />}
      {activeSection === "panic" && <PanicSection />}

        </div>
      </div>

      <ChangePasswordDialog
        open={changeOpen}
        onClose={() => setChangeOpen(false)}
        onChanged={async () => {
          setChangeOpen(false);
          await refresh();
        }}
      />
      </div>
    </main>
  );
}

// --- Updates section ----------------------------------------------------
//
// Two pieces of state the user controls:
//   1. Whether CloudSaw auto-checks on launch (persisted via
//      `updatePrefs` in localStorage; default ON). The UpdateBanner
//      reads the same flag and skips its on-mount check when off.
//   2. A manual "Check for updates" button that runs the same
//      `check()` from the Tauri updater plugin regardless of the
//      auto-check toggle, surfacing the available version + a link
//      to the GitHub release notes.

type UpdateCheckResult =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up_to_date"; at: string }
  | { kind: "available"; version: string; at: string }
  | { kind: "error"; message: string; at: string };

/** PR #54 — Notifications settings. Single user-facing toggle that
 *  gates the desktop notification fired on scan completion. The
 *  underlying helper (`lib/scanNotifications.ts`) handles
 *  permission prompts on first send; this section just exposes the
 *  user-controlled opt-in. */
/** PR #57: Settings → Appearance — Light / Dark / Match system. The
 *  hook handles persistence + applying the `dark` class to <html>;
 *  this component just owns the radio surface. */
function AppearanceSection() {
  const t = useT();
  const { appearance, setAppearance } = useAppearance();

  const options: { value: Appearance; label: string; description: string }[] = [
    {
      value: "light",
      label: t("settings.appearance.light.label"),
      description: t("settings.appearance.light.description"),
    },
    {
      value: "dark",
      label: t("settings.appearance.dark.label"),
      description: t("settings.appearance.dark.description"),
    },
    {
      value: "system",
      label: t("settings.appearance.system.label"),
      description: t("settings.appearance.system.description"),
    },
  ];

  return (
    <section
      className="max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6 dark:bg-saw-grey-dark dark:border-saw-grey-700"
      data-testid="settings-section-appearance"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("settings.appearance.title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-300">
        {t("settings.appearance.subtitle")}
      </p>
      <div
        className="mt-4 flex flex-col gap-3"
        role="radiogroup"
        aria-labelledby="settings-appearance-title"
      >
        {options.map((opt) => {
          const checked = appearance === opt.value;
          return (
            <label
              key={opt.value}
              className={
                "flex cursor-pointer items-start gap-3 rounded-card border p-3 transition " +
                (checked
                  ? "border-saw-red bg-saw-red/5 dark:bg-saw-red/10"
                  : "border-saw-grey-200 hover:bg-saw-grey-50 dark:border-saw-grey-700 dark:hover:bg-saw-grey-800")
              }
              data-testid={`settings-appearance-${opt.value}`}
            >
              <input
                type="radio"
                name="settings-appearance"
                value={opt.value}
                checked={checked}
                onChange={() => setAppearance(opt.value)}
                className="mt-1 accent-saw-red"
              />
              <div className="flex flex-col">
                <span className="text-body font-medium text-saw-grey-900 dark:text-saw-beige">
                  {opt.label}
                </span>
                <span className="text-small text-saw-grey-600 dark:text-saw-grey-300">
                  {opt.description}
                </span>
              </div>
            </label>
          );
        })}
      </div>
    </section>
  );
}

function NotificationsSection() {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean>(
    isScanNotificationsEnabled(),
  );

  function onToggle(next: boolean) {
    setEnabled(next);
    setScanNotificationsEnabled(next);
  }

  return (
    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-notifications"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("settings.notifications.title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("settings.notifications.subtitle")}
      </p>
      <div className="mt-4">
        <Switch
          label={t("settings.notifications.scan_complete_label")}
          description={t("settings.notifications.scan_complete_description")}
          checked={enabled}
          onChange={onToggle}
        />
      </div>
    </section>
  );
}

function UpdatesSection() {
  const t = useT();
  const [autoCheck, setAutoCheck] = useState<boolean>(true);
  const [installedVersion, setInstalledVersion] = useState<string | null>(null);
  const [result, setResult] = useState<UpdateCheckResult>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;
    void import("@/lib/updatePrefs").then(({ getAutoCheckEnabled }) => {
      if (cancelled) return;
      setAutoCheck(getAutoCheckEnabled());
    });
    void import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then((v) => {
        if (cancelled) return;
        setInstalledVersion(v);
      })
      .catch(() => {
        // In a non-Tauri context (e.g. the browser dev preview) the
        // import will reject. Leaving installedVersion null causes the
        // line to render an em-dash placeholder.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function persistAutoCheck(next: boolean) {
    setAutoCheck(next);
    const { setAutoCheckEnabled } = await import("@/lib/updatePrefs");
    setAutoCheckEnabled(next);
  }

  async function manualCheck() {
    setResult({ kind: "checking" });
    const at = new Date().toISOString();
    try {
      const { check: doCheck } = await import("@tauri-apps/plugin-updater");
      const update = await doCheck();
      if (!update) {
        setResult({ kind: "up_to_date", at });
        return;
      }
      setResult({ kind: "available", version: update.version, at });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Update check failed.";
      setResult({ kind: "error", message: msg, at });
    }
  }

  const lastCheckedLabel =
    result.kind === "idle" || result.kind === "checking"
      ? t("settings.updates.never_checked")
      : formatTimestamp(result.at);

  return (
    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-updates"
      aria-labelledby="settings-updates-title"
    >
      <h2
        id="settings-updates-title"
        className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige"
      >
        {t("settings.section.updates_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("settings.section.updates_subtitle")}
      </p>

      <div className="mt-4">
        <Switch
          checked={autoCheck}
          onChange={(next) => void persistAutoCheck(next)}
          label={t("settings.updates.auto_toggle_label")}
          description={t("settings.updates.auto_toggle_description")}
        />
      </div>

      <hr className="my-4 border-saw-grey-100 dark:border-saw-grey-800" />

      <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-small">
        <dt className="text-saw-grey-500 dark:text-saw-grey-400">
          {t("settings.updates.installed_version_label")}
        </dt>
        <dd
          className="font-mono text-saw-grey-900 dark:text-saw-beige"
          data-testid="settings-updates-installed-version"
        >
          {installedVersion ?? "—"}
        </dd>
        <dt className="text-saw-grey-500 dark:text-saw-grey-400">
          {t("settings.updates.last_checked_label")}
        </dt>
        <dd className="text-saw-grey-900 dark:text-saw-beige" data-testid="settings-updates-last-checked">
          {lastCheckedLabel}
        </dd>
      </dl>

      <div className="mt-4">
        <Button
          variant="secondary"
          onClick={() => void manualCheck()}
          disabled={result.kind === "checking"}
          data-testid="settings-updates-check"
        >
          {result.kind === "checking"
            ? t("settings.updates.checking")
            : t("settings.updates.check_cta")}
        </Button>
      </div>

      {result.kind === "up_to_date" ? (
        <p
          role="status"
          className="mt-4 rounded-card bg-saw-grey-50 dark:bg-saw-black px-3 py-2 text-small text-saw-grey-800 dark:text-saw-beige"
          data-testid="settings-updates-result-up-to-date"
        >
          {t("settings.updates.up_to_date")}
        </p>
      ) : null}

      {result.kind === "available" ? (
        <div
          role="status"
          className="mt-4 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-3 py-3 text-small text-saw-grey-800 dark:text-saw-beige"
          data-testid="settings-updates-result-available"
        >
          <p className="font-semibold text-saw-grey-900 dark:text-saw-beige">
            {t("settings.updates.available_title")}
          </p>
          <p className="mt-1">
            {t("settings.updates.available_body").replace(
              "{version}",
              result.version,
            )}
          </p>
          <p className="mt-2">
            <a
              href={`https://github.com/Camdun-Solutions/CloudSaw/releases/tag/${encodeURIComponent(result.version)}`}
              target="_blank"
              rel="noopener noreferrer"
              className="underline underline-offset-2"
              data-testid="settings-updates-release-notes-link"
            >
              {t("settings.updates.release_notes_link")}
            </a>
          </p>
        </div>
      ) : null}

      {result.kind === "error" ? (
        <div
          role="alert"
          className="mt-4 rounded-card border border-saw-red/30 bg-saw-red/5 px-3 py-3 text-small text-saw-grey-900 dark:text-saw-beige"
          data-testid="settings-updates-result-error"
        >
          <p className="font-semibold text-saw-red">
            {t("settings.updates.check_failed_title")}
          </p>
          <p className="mt-1 text-saw-grey-800 dark:text-saw-beige">
            {t("settings.updates.check_failed_body")}
          </p>
        </div>
      ) : null}
    </section>
  );
}

function formatTimestamp(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

// --- Contract 11 sections -----------------------------------------------

type RetentionChoice = "30d" | "60d" | "90d" | "180d" | "365d" | "never";

function periodToChoice(p: RetentionPeriod): RetentionChoice {
  if (p.kind === "never") return "never";
  switch (p.days) {
    case 30: return "30d";
    case 60: return "60d";
    case 90: return "90d";
    case 180: return "180d";
    case 365: return "365d";
    default: return "90d";
  }
}

function choiceToPeriod(c: RetentionChoice): RetentionPeriod {
  switch (c) {
    case "never": return { kind: "never" };
    case "30d": return { kind: "days", days: 30 };
    case "60d": return { kind: "days", days: 60 };
    case "90d": return { kind: "days", days: 90 };
    case "180d": return { kind: "days", days: 180 };
    case "365d": return { kind: "days", days: 365 };
  }
}

function RetentionSection() {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<RetentionSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setSettings(await ipc.retentionGetSettings());
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings) {
    return null;
  }

  const scanChoice = periodToChoice(settings.scan_retention);
  const eventChoice = periodToChoice(settings.eventlog_retention);

  const options: { value: RetentionChoice; label: string }[] = [
    { value: "30d", label: t("retention.period.30d") },
    { value: "60d", label: t("retention.period.60d") },
    { value: "90d", label: t("retention.period.90d") },
    { value: "180d", label: t("retention.period.180d") },
    { value: "365d", label: t("retention.period.365d") },
    { value: "never", label: t("retention.period.never") },
  ];

  async function updateScan(c: RetentionChoice) {
    setErr(null);
    try {
      await ipc.retentionSetScan(choiceToPeriod(c));
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function updateEventlog(c: RetentionChoice) {
    setErr(null);
    try {
      await ipc.retentionSetEventlog(choiceToPeriod(c));
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function runNow() {
    setBusy(true);
    setErr(null);
    setToast(null);
    try {
      const summary = await ipc.retentionRunNow();
      setToast(
        t("retention.toast")
          .replace("{scans}", String(summary.scan_dirs_removed))
          .replace("{raw}", String(summary.raw_files_removed))
          .replace("{events}", String(summary.eventlog_rows_removed)),
      );
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
      window.setTimeout(() => setToast(null), 4000);
    }
  }

  const lastRun = settings.last_run_at
    ? t("retention.last_run").replace("{at}", new Date(settings.last_run_at).toLocaleString())
    : t("retention.never_run");

  return (
    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-retention"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("retention.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("retention.section_subtitle")}
      </p>

      <div className="mt-4 flex flex-col gap-4">
        <Select<RetentionChoice>
          label={t("retention.scan.label")}
          description={t("retention.scan.hint")}
          value={scanChoice}
          options={options}
          onChange={(c) => void updateScan(c)}
          data-testid="settings-retention-scan"
        />
        <Select<RetentionChoice>
          label={t("retention.eventlog.label")}
          description={t("retention.eventlog.hint")}
          value={eventChoice}
          options={options}
          onChange={(c) => void updateEventlog(c)}
          data-testid="settings-retention-eventlog"
        />
        {(scanChoice === "never" || eventChoice === "never") ? (
          <p className="text-small text-saw-grey-600 dark:text-saw-grey-400">
            {t("retention.never_storage_hint")}
          </p>
        ) : null}
        <p className="text-small text-saw-grey-500 dark:text-saw-grey-400">{lastRun}</p>

        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
        {toast ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="settings-retention-toast"
          >
            {toast}
          </p>
        ) : null}

        <div>
          <Button
            variant="secondary"
            onClick={() => void runNow()}
            disabled={busy}
            data-testid="settings-retention-run"
          >
            {busy ? t("retention.run_busy") : t("retention.run_now")}
          </Button>
        </div>
      </div>
    </section>
  );
}

function PanicSection() {
  const t = useT();
  const formatError = useIpcError();
  // Panic-wipe modal state.
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<PanicWipeResult | null>(null);
  // PR #67: Reset Application modal state. Independent of panic
  // state so the two cards never share a confirmation buffer.
  const [resetOpen, setResetOpen] = useState(false);
  const [resetConfirm, setResetConfirm] = useState("");
  const [resetBusy, setResetBusy] = useState(false);
  const [resetErr, setResetErr] = useState<string | null>(null);

  function close() {
    setOpen(false);
    setConfirm("");
    setErr(null);
  }

  function closeReset() {
    // Only allow closing when we're not in the middle of the wipe
    // — the user explicitly asked us not to let the window be
    // closed during the reset.
    if (resetBusy) return;
    setResetOpen(false);
    setResetConfirm("");
    setResetErr(null);
  }

  async function doPanic() {
    if (confirm !== "PANIC") {
      setErr(t("eventlog.error.confirmation_rejected"));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      const out = await ipc.systemPanicWipe(confirm);
      setResult(out);
      setOpen(false);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
      setConfirm("");
    }
  }

  async function doReset() {
    if (resetConfirm !== "RESET") {
      setResetErr(t("eventlog.error.confirmation_rejected"));
      return;
    }
    setResetBusy(true);
    setResetErr(null);
    try {
      // The happy-path promise from systemResetApplication never
      // resolves — `AppHandle::restart()` kills the process. We
      // await it anyway so the spinner stays up until the process
      // is killed; the catch branch handles confirmation-rejected
      // or write failures that come back BEFORE restart.
      await ipc.systemResetApplication(resetConfirm);
    } catch (e) {
      setResetErr(formatError(e));
      setResetBusy(false);
    }
  }

  async function doReboot() {
    try {
      await ipc.systemRequestReboot();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setResult(null);
    }
  }

  return (
    <>
      {/* PR #67: Reset Application — sits ABOVE the Panic card so a
          user who just wants a fresh-install experience finds the
          less destructive option first. */}
      <section
        className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
        data-testid="settings-section-reset"
      >
        <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
          {t("reset.section_title")}
        </h2>
        <p className="mt-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          {t("reset.section_subtitle")}
        </p>
        <p className="mt-2 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("reset.section_warning")}
        </p>
        <div className="mt-4">
          <Button
            variant="primary"
            onClick={() => setResetOpen(true)}
            data-testid="settings-reset-cta"
          >
            {t("reset.section_cta")}
          </Button>
        </div>
      </section>

      <section
        className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-red/40 p-6"
        data-testid="settings-section-panic"
      >
        <h2 className="text-h3 font-semibold text-saw-red">{t("panic.section_title")}</h2>
        <p className="mt-1 text-small text-saw-grey-700 dark:text-saw-grey-300">{t("panic.section_subtitle")}</p>
        <div className="mt-4">
          <Button
            variant="primary"
            onClick={() => setOpen(true)}
            data-testid="settings-panic-cta"
          >
            {t("panic.section_cta")}
          </Button>
        </div>
      </section>

      <Modal
        open={open}
        onClose={close}
        title={t("panic.title")}
        footer={
          <>
            <Button variant="ghost" onClick={close} disabled={busy}>
              {t("panic.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={() => void doPanic()}
              disabled={busy || confirm !== "PANIC"}
              data-testid="panic-confirm"
            >
              {busy ? t("panic.busy") : t("panic.confirm_cta")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <p>{t("panic.explainer")}</p>
          <p className="text-small text-saw-red">{t("panic.warning")}</p>
          <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
            <span>{t("panic.confirm_label")}</span>
            <input
              type="text"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              placeholder={t("panic.confirm_placeholder")}
              autoFocus
              className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
              data-testid="panic-confirm-input"
            />
          </label>
          {err ? (
            <p role="alert" className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red">
              {err}
            </p>
          ) : null}
        </div>
      </Modal>

      {result ? (
        <Modal
          open={!!result}
          onClose={() => setResult(null)}
          title={t("panic.success.title")}
          footer={
            <>
              <Button
                variant="ghost"
                onClick={() => setResult(null)}
                data-testid="panic-later"
              >
                {t("panic.success.later")}
              </Button>
              <Button
                variant="primary"
                onClick={() => void doReboot()}
                data-testid="panic-reboot-now"
              >
                {t("panic.success.reboot_now")}
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-3">
            <p>
              {t("panic.success.body")
                .replace("{scans}", String(result.scan_dirs_removed))
                .replace("{tf}", String(result.tf_workdirs_removed))
                .replace("{logs}", String(result.log_files_removed))
                .replace("{dbs}", String(result.db_files_removed))
                .replace("{keychain}", String(result.keychain.removed))
                .replace(
                  "{staged}",
                  result.self_delete_staged
                    ? t("panic.success.staged_yes")
                    : t("panic.success.staged_no"),
                )}
            </p>
            <p className="text-small text-saw-grey-700 dark:text-saw-grey-300">
              {t("panic.success.reboot_question")}
            </p>
          </div>
        </Modal>
      ) : null}

      {/* PR #67: Reset Application confirmation modal. While
          resetBusy is true the modal cannot be closed and shows a
          plain "Resetting…" body — the user was warned not to close
          the window. The app restarts via `AppHandle::restart()`
          inside the IPC so the modal will simply disappear with the
          rest of the process. */}
      <Modal
        open={resetOpen}
        onClose={closeReset}
        title={t("reset.modal.title")}
        footer={
          resetBusy ? null : (
            <>
              <Button
                variant="ghost"
                onClick={closeReset}
                disabled={resetBusy}
                data-testid="reset-cancel"
              >
                {t("reset.modal.cancel")}
              </Button>
              <Button
                variant="primary"
                onClick={() => void doReset()}
                disabled={resetBusy || resetConfirm !== "RESET"}
                data-testid="reset-confirm"
              >
                {t("reset.modal.confirm_cta")}
              </Button>
            </>
          )
        }
      >
        {resetBusy ? (
          <div className="flex flex-col gap-3" data-testid="reset-busy">
            <p className="text-body font-medium text-saw-grey-900 dark:text-saw-beige">
              {t("reset.modal.busy")}
            </p>
            <p className="text-small text-saw-grey-700 dark:text-saw-grey-300">
              {t("reset.modal.busy_body")}
            </p>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <p>{t("reset.modal.explainer")}</p>
            <p className="text-small text-saw-red">
              {t("reset.modal.warning")}
            </p>
            <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
              <span>{t("reset.modal.confirm_label")}</span>
              <input
                type="text"
                value={resetConfirm}
                onChange={(e) => setResetConfirm(e.target.value)}
                autoFocus
                className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
                data-testid="reset-confirm-input"
              />
            </label>
            {resetErr ? (
              <p
                role="alert"
                className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
                data-testid="reset-error"
              >
                {resetErr}
              </p>
            ) : null}
          </div>
        )}
      </Modal>
    </>
  );
}

const MIN_PASSWORD_LEN = 8;

function ChangePasswordDialog({
  open,
  onClose,
  onChanged,
}: {
  open: boolean;
  onClose: () => void;
  onChanged: () => Promise<void>;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setOldPw("");
      setNewPw("");
      setConfirm("");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const tooShort = newPw.length > 0 && newPw.length < MIN_PASSWORD_LEN;
  const mismatch = newPw.length > 0 && confirm.length > 0 && newPw !== confirm;
  const canSubmit =
    !busy &&
    oldPw.length > 0 &&
    newPw.length >= MIN_PASSWORD_LEN &&
    newPw === confirm;

  async function onSubmit() {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      await ipc.applockChangePassword(oldPw, newPw);
      setOldPw("");
      setNewPw("");
      setConfirm("");
      await onChanged();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t("applock.settings.change_password")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onSubmit}
            disabled={!canSubmit}
            data-testid="change-password-submit"
          >
            {busy ? t("applock.recovery.busy") : t("applock.settings.change_password")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {/* QA FINDING-005: required + minLength attrs reinforce the
            JS-side validation. They don't fire native form validation
            here (no <form onSubmit>), but they let password managers
            and accessibility tools read the field requirements, and
            they'll engage automatically if a future change wraps the
            dialog in a form. */}
        <PasswordField
          label={t("applock.field.old_password")}
          name="current-password"
          required
          value={oldPw}
          onChange={(e) => setOldPw(e.target.value)}
          autoComplete="current-password"
          showLabel={t("applock.field.show")}
          hideLabel={t("applock.field.hide")}
        />
        <PasswordField
          label={t("applock.field.new_password")}
          name="new-password"
          required
          minLength={MIN_PASSWORD_LEN}
          value={newPw}
          onChange={(e) => setNewPw(e.target.value)}
          autoComplete="new-password"
          hint={t("applock.setup.password_hint").replace(
            "{min}",
            String(MIN_PASSWORD_LEN),
          )}
          error={tooShort ? t("applock.error.too_short") : null}
          showLabel={t("applock.field.show")}
          hideLabel={t("applock.field.hide")}
        />
        <PasswordField
          label={t("applock.field.confirm_password")}
          name="confirm-password"
          required
          minLength={MIN_PASSWORD_LEN}
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          autoComplete="new-password"
          error={mismatch ? t("applock.error.mismatch") : null}
          showLabel={t("applock.field.show")}
          hideLabel={t("applock.field.hide")}
        />
        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
          >
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}

// --- GitHub integration (Contract 12) -----------------------------------

function GithubSection() {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<GithubSettings | null>(null);
  const [tokenInput, setTokenInput] = useState("");
  const [tokenBusy, setTokenBusy] = useState(false);
  const [tokenSaved, setTokenSaved] = useState(false);
  const [repoInput, setRepoInput] = useState("");
  const [repoBusy, setRepoBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const s = await ipc.githubGetSettings();
      setSettings(s);
      setRepoInput(s.findings_repo ? `${s.findings_repo.owner}/${s.findings_repo.name}` : "");
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function saveToken() {
    setErr(null);
    setTokenBusy(true);
    setTokenSaved(false);
    try {
      await ipc.githubSetToken(tokenInput);
      setTokenInput("");
      setTokenSaved(true);
      window.setTimeout(() => setTokenSaved(false), 3000);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setTokenBusy(false);
    }
  }
  async function clearToken() {
    setErr(null);
    try {
      await ipc.githubClearToken();
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function saveRepo() {
    setErr(null);
    setRepoBusy(true);
    try {
      const parts = repoInput.trim().split("/");
      if (parts.length !== 2 || !parts[0] || !parts[1]) {
        setErr(t("github.error.no_findings_repo"));
        return;
      }
      await ipc.githubSetFindingsRepo({ owner: parts[0], name: parts[1] });
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setRepoBusy(false);
    }
  }
  async function clearRepo() {
    setErr(null);
    try {
      await ipc.githubSetFindingsRepo(null);
      setRepoInput("");
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function openTokenPage() {
    // PR #68: WebView2 blocks JS-initiated `window.open()` calls, so
    // route the URL through `@tauri-apps/plugin-opener` instead. The
    // Rust handler returns the PAT-creation URL string; `openUrl`
    // hands it to the OS default browser.
    try {
      const url = await ipc.githubGenerateTokenUrl();
      await openUrl(url);
    } catch (e) {
      setErr(formatError(e));
    }
  }

  if (!settings) return null;

  return (
    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-github"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("github.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("github.section_subtitle")}
      </p>

      <div className="mt-4 flex flex-col gap-4">
        <p
          className="text-small text-saw-grey-700 dark:text-saw-grey-300"
          data-testid="settings-github-token-status"
        >
          {settings.token.configured
            ? t("github.token.configured")
            : t("github.token.not_configured")}
        </p>
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("github.token.label")}</span>
          <input
            type="password"
            value={tokenInput}
            onChange={(e) => setTokenInput(e.target.value)}
            placeholder={t("github.token.placeholder")}
            autoComplete="off"
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige font-mono"
            data-testid="settings-github-token-input"
          />
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">{t("github.token.hint")}</span>
        </label>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            onClick={() => void saveToken()}
            disabled={tokenBusy || tokenInput.trim().length === 0}
            data-testid="settings-github-token-save"
          >
            {tokenBusy ? t("github.token.saving") : t("github.token.save")}
          </Button>
          <Button
            variant="ghost"
            onClick={() => void openTokenPage()}
            data-testid="settings-github-generate"
          >
            {t("github.token.generate_cta")}
          </Button>
          {settings.token.configured ? (
            <Button
              variant="ghost"
              onClick={() => void clearToken()}
              data-testid="settings-github-token-clear"
            >
              {t("github.token.clear")}
            </Button>
          ) : null}
        </div>
        {tokenSaved ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="settings-github-token-saved"
          >
            {t("github.token.configured")}
          </p>
        ) : null}

        <hr className="border-saw-grey-100 dark:border-saw-grey-800" />

        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("github.findings_repo.label")}</span>
          <input
            type="text"
            value={repoInput}
            onChange={(e) => setRepoInput(e.target.value)}
            placeholder={t("github.findings_repo.placeholder")}
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige font-mono"
            data-testid="settings-github-repo-input"
          />
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">{t("github.findings_repo.hint")}</span>
        </label>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            onClick={() => void saveRepo()}
            disabled={repoBusy || repoInput.trim().length === 0}
            data-testid="settings-github-repo-save"
          >
            {t("github.findings_repo.save")}
          </Button>
          {settings.findings_repo ? (
            <Button
              variant="ghost"
              onClick={() => void clearRepo()}
              data-testid="settings-github-repo-clear"
            >
              {t("github.findings_repo.clear")}
            </Button>
          ) : null}
        </div>
        {!settings.findings_repo ? (
          <p className="text-small text-saw-grey-500 dark:text-saw-grey-400" data-testid="settings-github-repo-none">
            {t("github.findings_repo.none")}
          </p>
        ) : null}

        {/* PR #68: "Error-report destination" + "Security contact"
            blocks removed. Bug reports now flow through the
            ReportBugFlag at the bottom-left of every screen, which
            opens a modal with the GitHub-issues link + the
            mailto:security@cloud-saw.com link directly. */}

        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
      </div>
    </section>
  );
}

// --- AI Suggestion Layer (Contract 13) ----------------------------------

/** PR #77 — placeholder for the API-key input, dispatched on the
 * provider type. Centralizes the per-provider shape hint so the
 * Add and Edit modals stay in sync if a provider's key format
 * changes. */
function keyPlaceholderFor(
  provider: AiProvider,
  t: (k: string) => string,
): string {
  switch (provider) {
    case "anthropic":
      return t("ai.key.placeholder_anthropic");
    case "openai":
      return t("ai.key.placeholder_openai");
    case "gemini":
      return t("ai.key.placeholder_gemini");
  }
}

/** PR #74 — Add Provider modal. Three fields: Provider Type +
 * Nickname + API Key. The key never re-renders after submit; the
 * keychain owns it from here on. */
function AddProviderModal({
  open,
  onClose,
  onSaved,
}: {
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [providerType, setProviderType] = useState<AiProvider>("anthropic");
  const [nickname, setNickname] = useState("");
  const [keyInput, setKeyInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Reset state every time the modal opens so a previous attempt
  // doesn't leak its (rejected) key into the next session.
  useEffect(() => {
    if (open) {
      setProviderType("anthropic");
      setNickname("");
      setKeyInput("");
      setBusy(false);
      setErr(null);
    }
  }, [open]);

  async function submit() {
    setErr(null);
    if (!nickname.trim()) {
      setErr(t("ai.providers.error.no_nickname"));
      return;
    }
    if (!keyInput.trim()) {
      setErr(t("ai.providers.error.no_key"));
      return;
    }
    setBusy(true);
    try {
      await ipc.aiAddProvider(providerType, nickname.trim(), keyInput);
      setKeyInput("");
      onSaved();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  if (!open) return null;
  return (
    <Modal
      open
      onClose={onClose}
      title={t("ai.providers.add_title")}
      data-testid="ai-provider-add-modal"
    >
      <div className="flex flex-col gap-4">
        {/* PR #77 — provider dropdown uses the shared Select so the
            modern dropdown style (custom popup, rounded items) is
            consistent with Compliance Obligations / Risk / Team /
            etc. Gemini joins Anthropic and OpenAI in the option set. */}
        <Select<AiProvider>
          label={t("ai.provider.label")}
          value={providerType}
          options={[
            { value: "anthropic", label: t("ai.provider.anthropic") },
            { value: "openai", label: t("ai.provider.openai") },
            { value: "gemini", label: t("ai.provider.gemini") },
          ]}
          onChange={setProviderType}
          data-testid="ai-provider-add-type"
        />
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("ai.providers.nickname")}</span>
          <input
            type="text"
            value={nickname}
            onChange={(e) => setNickname(e.target.value.slice(0, 60))}
            placeholder={t("ai.providers.nickname_placeholder")}
            maxLength={60}
            className="rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
            data-testid="ai-provider-add-nickname"
          />
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("ai.providers.nickname_hint")}
          </span>
        </label>
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("ai.key.label")}</span>
          <input
            type="password"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder={keyPlaceholderFor(providerType, t)}
            autoComplete="off"
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige font-mono"
            data-testid="ai-provider-add-key"
          />
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("ai.key.hint")}
          </span>
        </label>
        {err ? (
          <p role="alert" className="text-small text-saw-red">
            {err}
          </p>
        ) : null}
        <div className="mt-2 flex justify-end gap-2">
          <Button
            variant="ghost"
            onClick={onClose}
            data-testid="ai-provider-add-cancel"
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={busy || !nickname.trim() || !keyInput.trim()}
            data-testid="ai-provider-add-submit"
          >
            {busy ? t("ai.key.saving") : t("ai.providers.add")}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** PR #74 — Edit Provider modal. Lets the user rename the row and/or
 * paste a new key. Both fields are optional; the IPC accepts `null`
 * for "don't touch." The provider type can NOT be changed (it's the
 * shape of the API the keychain key satisfies). */
function EditProviderModal({
  provider,
  onClose,
  onSaved,
}: {
  provider: ProviderRecord | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [nickname, setNickname] = useState("");
  const [keyInput, setKeyInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (provider) {
      setNickname(provider.nickname);
      setKeyInput("");
      setBusy(false);
      setErr(null);
    }
  }, [provider]);

  if (!provider) return null;

  async function submit() {
    if (!provider) return;
    setErr(null);
    const trimmedNick = nickname.trim();
    if (!trimmedNick) {
      setErr(t("ai.providers.error.no_nickname"));
      return;
    }
    const nicknameChanged = trimmedNick !== provider.nickname;
    const keyChanged = keyInput.trim().length > 0;
    if (!nicknameChanged && !keyChanged) {
      onClose();
      return;
    }
    setBusy(true);
    try {
      await ipc.aiUpdateProvider(
        provider.provider_id,
        nicknameChanged ? trimmedNick : null,
        keyChanged ? keyInput : null,
      );
      setKeyInput("");
      onSaved();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={t("ai.providers.edit_title")}
      data-testid="ai-provider-edit-modal"
    >
      <div className="flex flex-col gap-4">
        <div className="text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t(`ai.provider.${provider.provider_type}`)} · ****
          {provider.key_last4 || "XXXX"}
        </div>
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("ai.providers.nickname")}</span>
          <input
            type="text"
            value={nickname}
            onChange={(e) => setNickname(e.target.value.slice(0, 60))}
            maxLength={60}
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
            data-testid="ai-provider-edit-nickname"
          />
        </label>
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("ai.providers.new_key")}</span>
          <input
            type="password"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder={t("ai.providers.new_key_placeholder")}
            autoComplete="off"
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige font-mono"
            data-testid="ai-provider-edit-key"
          />
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("ai.providers.new_key_hint")}
          </span>
        </label>
        {err ? (
          <p role="alert" className="text-small text-saw-red">
            {err}
          </p>
        ) : null}
        <div className="mt-2 flex justify-end gap-2">
          <Button
            variant="ghost"
            onClick={onClose}
            data-testid="ai-provider-edit-cancel"
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={busy}
            data-testid="ai-provider-edit-submit"
          >
            {busy ? t("ai.key.saving") : t("common.save")}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

function AiSection() {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<AiSettingsT | null>(null);
  // PR #74 — multi-provider list. Renders one row per connected
  // provider; each row has a meatball menu with Edit / Delete.
  const [providers, setProviders] = useState<ProviderRecord[]>([]);
  const [openMenuId, setOpenMenuId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [editProvider, setEditProvider] = useState<ProviderRecord | null>(null);
  const [deleteProvider, setDeleteProvider] = useState<ProviderRecord | null>(
    null,
  );
  const [context, setContext] = useState<BusinessContext | null>(null);
  const [ctxSaved, setCtxSaved] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // PR #71: disclosure body starts collapsed so the rest of the AI
  // tab is reachable without scrolling past the full warning. The
  // first ~3 lines + the title stay visible, with a Read more /
  // Read less inline toggle.
  const [disclosureOpen, setDisclosureOpen] = useState(false);

  const reload = useCallback(async () => {
    try {
      const [s, list] = await Promise.all([
        ipc.aiGetSettings(),
        ipc.aiListProviders(),
      ]);
      setSettings(s);
      setProviders(list);
      setContext(s.context);
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings || !context) return null;

  async function setActive(providerId: string) {
    setErr(null);
    setOpenMenuId(null);
    try {
      await ipc.aiSetActiveProvider(providerId);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }

  async function confirmDelete() {
    if (!deleteProvider) return;
    setErr(null);
    try {
      await ipc.aiDeleteProvider(deleteProvider.provider_id);
      setDeleteProvider(null);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function saveContext() {
    setErr(null);
    if (!context) return;
    setCtxSaved(false);
    try {
      // PR #69: compliance is now stored directly on the context
      // (managed by the TagInput pill editor) instead of a
      // comma-delimited string buffer.
      await ipc.aiSetBusinessContext(context);
      setCtxSaved(true);
      window.setTimeout(() => setCtxSaved(false), 3000);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }

  const envOptions: { value: EnvironmentType; label: string }[] = [
    { value: "unspecified", label: t("ai.context.env.unspecified") },
    { value: "production", label: t("ai.context.env.production") },
    { value: "dev_test", label: t("ai.context.env.dev_test") },
    { value: "mixed", label: t("ai.context.env.mixed") },
  ];
  const riskOptions: { value: RiskTolerance; label: string }[] = [
    { value: "unspecified", label: t("ai.context.risk.unspecified") },
    { value: "low", label: t("ai.context.risk.low") },
    { value: "medium", label: t("ai.context.risk.medium") },
    { value: "high", label: t("ai.context.risk.high") },
  ];
  const teamOptions: { value: TeamSize; label: string }[] = [
    { value: "unspecified", label: t("ai.context.team.unspecified") },
    { value: "solo", label: t("ai.context.team.solo") },
    { value: "small", label: t("ai.context.team.small") },
    { value: "medium", label: t("ai.context.team.medium") },
    { value: "large", label: t("ai.context.team.large") },
  ];

  return (
    <>
    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-ai"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("ai.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("ai.section_subtitle")}
      </p>

      {!settings.key_connected ? (
        <div
          className="mt-4 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black p-3 text-small"
          data-testid="ai-dormant-note"
        >
          <div className="font-medium text-saw-grey-900 dark:text-saw-beige">
            {t("ai.dormant.title")}
          </div>
          <div className="text-saw-grey-700 dark:text-saw-grey-300 mt-1">{t("ai.dormant.body")}</div>
        </div>
      ) : null}

      {/* PR #71: the disclosure body is multi-paragraph long and used
          to dominate the entire AI tab. It now collapses to its
          first ~3 lines (line-clamp-3) with an inline "Read more"
          toggle so the rest of the page is reachable without
          scrolling past a wall of red text. */}
      <div className="mt-4 rounded-card border border-saw-red/30 bg-saw-red/5 p-3 text-small">
        <div className="font-medium text-saw-red">{t("ai.disclosure.title")}</div>
        <div
          className={`text-saw-grey-800 dark:text-saw-beige mt-1 ${
            disclosureOpen ? "" : "line-clamp-3"
          }`}
          data-testid="ai-disclosure-body"
        >
          {t("ai.disclosure.body")}
        </div>
        <button
          type="button"
          onClick={() => setDisclosureOpen((v) => !v)}
          data-testid="ai-disclosure-toggle"
          className="mt-1 text-saw-red underline underline-offset-2 hover:text-saw-red-bold focus:outline-none focus-visible:ring-2 focus-visible:ring-saw-red"
        >
          {disclosureOpen
            ? t("ai.disclosure.read_less")
            : t("ai.disclosure.read_more")}
        </button>
      </div>

      <div className="mt-4 flex flex-col gap-4">
        {/* PR #74 — Connected Providers list. Each row shows
            "{nickname} ({provider_type})" + ****XXXX + an active
            indicator. The meatball menu offers Edit (modal, nickname
            and/or new key) and Delete (red destructive). "Add
            provider" opens the new-provider modal with the same
            shape. The real key only ever lives in the keychain — the
            ipc surface returns `key_last4` only. */}
        <div className="flex items-center justify-between">
          <div>
            <div className="font-medium text-saw-grey-900 dark:text-saw-beige">
              {t("ai.providers.title")}
            </div>
            <div className="text-small text-saw-grey-600 dark:text-saw-grey-400 mt-1">
              {t("ai.providers.subtitle")}
            </div>
          </div>
          <Button
            variant="primary"
            onClick={() => setAddOpen(true)}
            data-testid="ai-provider-add"
          >
            {t("ai.providers.add")}
          </Button>
        </div>
        {providers.length === 0 ? (
          <div
            className="rounded-card border border-dashed border-saw-grey-300 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black p-4 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="ai-providers-empty"
          >
            {t("ai.providers.empty")}
          </div>
        ) : (
          <ul
            className="flex flex-col gap-2"
            data-testid="ai-providers-list"
          >
            {providers.map((p) => (
              <li
                key={p.provider_id}
                className="relative flex items-center justify-between gap-3 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-4 py-3"
                data-testid={`ai-provider-row-${p.provider_id}`}
              >
                <div className="flex flex-col gap-0.5 min-w-0 flex-1">
                  <div className="flex items-center gap-2 text-body text-saw-grey-900 dark:text-saw-beige">
                    <span className="font-medium truncate">{p.nickname}</span>
                    <span className="text-small text-saw-grey-500 dark:text-saw-grey-400">
                      ({t(`ai.provider.${p.provider_type}`)})
                    </span>
                    {p.is_active ? (
                      <span
                        className="ml-1 inline-flex items-center rounded-pill bg-saw-grey-100 dark:bg-saw-grey-800 px-2 py-0.5 text-xs font-medium text-saw-grey-700 dark:text-saw-grey-300"
                        data-testid={`ai-provider-active-${p.provider_id}`}
                      >
                        {t("ai.providers.active")}
                      </span>
                    ) : null}
                  </div>
                  <div className="text-small font-mono text-saw-grey-500 dark:text-saw-grey-400">
                    ****{p.key_last4 || "XXXX"}
                  </div>
                </div>
                {!p.is_active ? (
                  <Button
                    variant="ghost"
                    onClick={() => void setActive(p.provider_id)}
                    data-testid={`ai-provider-activate-${p.provider_id}`}
                  >
                    {t("ai.providers.set_active")}
                  </Button>
                ) : null}
                <div className="relative">
                  <button
                    type="button"
                    aria-label={t("ai.providers.actions")}
                    onClick={() =>
                      setOpenMenuId(
                        openMenuId === p.provider_id ? null : p.provider_id,
                      )
                    }
                    className="rounded-card p-2 text-saw-grey-600 dark:text-saw-grey-300 hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800 focus:outline-none focus:ring-2 focus:ring-saw-red"
                    data-testid={`ai-provider-menu-${p.provider_id}`}
                  >
                    {/* Meatball glyph — three horizontal dots. */}
                    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
                      <circle cx="3" cy="8" r="1.5" fill="currentColor" />
                      <circle cx="8" cy="8" r="1.5" fill="currentColor" />
                      <circle cx="13" cy="8" r="1.5" fill="currentColor" />
                    </svg>
                  </button>
                  {openMenuId === p.provider_id ? (
                    <div
                      role="menu"
                      className="absolute right-0 top-full mt-1 z-10 min-w-[140px] rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark shadow-lg"
                      data-testid={`ai-provider-menu-popup-${p.provider_id}`}
                    >
                      <button
                        type="button"
                        role="menuitem"
                        onClick={() => {
                          setEditProvider(p);
                          setOpenMenuId(null);
                        }}
                        className="block w-full text-left px-4 py-2 text-small text-saw-grey-900 dark:text-saw-beige hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800"
                        data-testid={`ai-provider-edit-${p.provider_id}`}
                      >
                        {t("ai.providers.edit")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        onClick={() => {
                          setDeleteProvider(p);
                          setOpenMenuId(null);
                        }}
                        className="block w-full text-left px-4 py-2 text-small text-saw-red hover:bg-saw-red/5"
                        data-testid={`ai-provider-delete-${p.provider_id}`}
                      >
                        {t("ai.providers.delete")}
                      </button>
                    </div>
                  ) : null}
                </div>
              </li>
            ))}
          </ul>
        )}

        <AddProviderModal
          open={addOpen}
          onClose={() => setAddOpen(false)}
          onSaved={() => {
            setAddOpen(false);
            void reload();
          }}
        />
        <EditProviderModal
          provider={editProvider}
          onClose={() => setEditProvider(null)}
          onSaved={() => {
            setEditProvider(null);
            void reload();
          }}
        />
        {deleteProvider ? (
          <Modal
            open
            onClose={() => setDeleteProvider(null)}
            title={t("ai.providers.delete_title")}
            data-testid="ai-provider-delete-modal"
          >
            <p className="text-body text-saw-grey-800 dark:text-saw-beige">
              {t("ai.providers.delete_confirm").replace(
                "{nickname}",
                deleteProvider.nickname,
              )}
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <Button
                variant="ghost"
                onClick={() => setDeleteProvider(null)}
                data-testid="ai-provider-delete-cancel"
              >
                {t("common.cancel")}
              </Button>
              <Button
                variant="danger"
                onClick={() => void confirmDelete()}
                data-testid="ai-provider-delete-confirm"
              >
                {t("ai.providers.delete")}
              </Button>
            </div>
          </Modal>
        ) : null}
      </div>
    </section>

    {/* PR #71: Business Context lives in its own card now. The
        original single-card layout collapsed the AI Suggestion
        Layer settings and the (longer) business-context form into
        one wall; splitting them lets a user fill out the context
        independently of whether a provider key is connected. */}
    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-ai-context"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("ai.context.title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("ai.context.subtitle")}
      </p>
      <div className="mt-4 flex flex-col gap-4">
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("ai.context.industry")}</span>
          <input
            type="text"
            value={context.industry}
            onChange={(e) =>
              setContext({ ...context, industry: e.target.value })
            }
            placeholder={t("ai.context.industry_placeholder")}
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
            data-testid="ai-ctx-industry"
          />
          {/* PR #69: identifying-content warning hint removed. */}
        </label>

        {/* PR #69: Job role textarea, capped at 500 chars. Sits
            directly below the Industry field. */}
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("ai.context.job_role")}</span>
          <textarea
            value={context.job_role}
            onChange={(e) =>
              setContext({
                ...context,
                job_role: e.target.value.slice(0, JOB_ROLE_MAX_LEN),
              })
            }
            placeholder={t("ai.context.job_role_placeholder")}
            maxLength={JOB_ROLE_MAX_LEN}
            rows={3}
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
            data-testid="ai-ctx-job-role"
          />
          <span
            className="flex items-center justify-between text-xs text-saw-grey-500 dark:text-saw-grey-400"
            data-testid="ai-ctx-job-role-meta"
          >
            <span>{t("ai.context.job_role_hint")}</span>
            <span>
              {context.job_role.length}/{JOB_ROLE_MAX_LEN}
            </span>
          </span>
        </label>

        {/* PR #77 — Environment/Risk/Team dropdowns moved from native
            <select> to the shared Select component so they share the
            modern dropdown style with Compliance Obligations'
            suggestion list (rounded-card panel, hover state,
            keyboard nav). Each is still a single-select; the visual
            language is the only thing that's converging. */}
        <Select<EnvironmentType>
          label={t("ai.context.environment")}
          value={context.environment_type}
          options={envOptions}
          onChange={(next) =>
            setContext({ ...context, environment_type: next })
          }
          data-testid="ai-ctx-env"
        />

        {/* PR #69: Compliance obligations are now a pill editor.
            Typing surfaces suggestions from KNOWN_COMPLIANCE_FRAMEWORKS
            (US + EU + Asia). Enter / comma / clicking a suggestion
            adds the value as a pill; custom values not in the list
            are accepted via the comma delimiter. The identifying-
            content warning was removed per user spec. */}
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("ai.context.compliance")}</span>
          <TagInput
            value={context.compliance}
            onChange={(next) =>
              setContext({ ...context, compliance: next })
            }
            suggestions={KNOWN_COMPLIANCE_FRAMEWORKS}
            placeholder={t("ai.context.compliance_placeholder")}
            maxTags={20}
            maxTagLength={40}
            data-testid="ai-ctx-compliance"
          />
        </label>

        <Select<RiskTolerance>
          label={t("ai.context.risk")}
          value={context.risk_tolerance}
          options={riskOptions}
          onChange={(next) =>
            setContext({ ...context, risk_tolerance: next })
          }
          data-testid="ai-ctx-risk"
        />

        <Select<TeamSize>
          label={t("ai.context.team")}
          value={context.team_size}
          options={teamOptions}
          onChange={(next) => setContext({ ...context, team_size: next })}
          data-testid="ai-ctx-team"
        />

        <div>
          <Button
            variant="primary"
            onClick={() => void saveContext()}
            data-testid="ai-ctx-save"
          >
            {t("ai.context.save")}
          </Button>
        </div>
        {ctxSaved ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="ai-ctx-saved"
          >
            {t("ai.context.saved")}
          </p>
        ) : null}

        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
      </div>
    </section>
    </>
  );
}

// --- Report exporter section (Contract 15) -----------------------------

function ReportSection() {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<ReportSettingsT | null>(null);
  const [busy, setBusy] = useState(false);
  const [pickerBusy, setPickerBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // PR #69: Custom report is now a modal inside the Reports tab,
  // not a separate route.
  const [customOpen, setCustomOpen] = useState(false);

  const reload = useCallback(async () => {
    try {
      setSettings(await ipc.reportGetSettings());
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings) return null;

  async function chooseFolder() {
    setPickerBusy(true);
    setErr(null);
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (picked && typeof picked === "string") {
        setSettings({ ...settings!, auto_export_folder: picked });
      }
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setPickerBusy(false);
    }
  }

  async function persist() {
    setBusy(true);
    setSaved(false);
    setErr(null);
    try {
      await ipc.reportSetSettings(settings!);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2500);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
    {/* PR #69: Custom report gets its own subsection above the
        auto-export settings. Title + description + a single
        primary CTA that opens the modal. */}
    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-custom-report"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("report.custom.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("report.custom.section_subtitle")}
      </p>
      <div className="mt-4">
        <Button
          variant="primary"
          onClick={() => setCustomOpen(true)}
          data-testid="settings-open-custom-report"
        >
          {t("report.custom.cta")}
        </Button>
      </div>
    </section>

    <section
      className="max-w-2xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-report"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("report.settings.title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
        {t("report.settings.subtitle")}
      </p>

      <div className="mt-4 flex flex-col gap-4">
        <label className="flex items-start gap-2 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <input
            type="checkbox"
            checked={settings.auto_export_enabled}
            onChange={(e) =>
              setSettings({ ...settings, auto_export_enabled: e.target.checked })
            }
            className="mt-1"
            data-testid="settings-reports-enable"
          />
          <span>{t("report.settings.enable")}</span>
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("report.settings.folder_label")}</span>
          <input
            type="text"
            readOnly
            value={settings.auto_export_folder ?? ""}
            placeholder={t("report.settings.folder_placeholder")}
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige font-mono"
            data-testid="settings-reports-folder"
          />
        </label>
        <div>
          <Button
            variant="secondary"
            onClick={() => void chooseFolder()}
            disabled={pickerBusy}
            data-testid="settings-reports-choose-folder"
          >
            {t("report.settings.choose_folder")}
          </Button>
        </div>

        <label className="flex items-start gap-2 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <input
            type="checkbox"
            checked={settings.mask_account_ids_default}
            onChange={(e) =>
              setSettings({ ...settings, mask_account_ids_default: e.target.checked })
            }
            className="mt-1"
            data-testid="settings-reports-mask-default"
          />
          <span>{t("report.settings.mask_default")}</span>
        </label>

        <div>
          <Button
            variant="primary"
            onClick={() => void persist()}
            disabled={busy}
            data-testid="settings-reports-save"
          >
            {t("report.settings.save")}
          </Button>
        </div>
        {saved ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="settings-reports-saved"
          >
            {t("report.settings.saved")}
          </p>
        ) : null}
        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
      </div>
    </section>

    <CustomReportModal
      open={customOpen}
      onClose={() => setCustomOpen(false)}
    />
    </>
  );
}
