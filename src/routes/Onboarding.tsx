// Onboarding wizard — Contract 14.
//
// On first launch (and whenever `OnboardingState.completed = false`)
// the App shell renders ONLY this route. The wizard is responsible
// for taking the user through six steps; no main-app route is
// reachable until `onboardingComplete()` is invoked.
//
// Each step:
//   * Renders its own primary action (saves through the relevant
//     contract's IPC) AND a secondary "I'll do this myself" toggle
//     that exposes the equivalent CLI commands. We NEVER execute the
//     CLI commands on the user's behalf (Contract 14 §Constraints).
//   * Marks itself completed via `onboardingMarkStepCompleted`.
//   * Advances ONLY when the user clicks the explicit "Next step"
//     button — no auto-advance.
//
// State is hydrated from the backend on every step transition so a
// quit-and-relaunch lands the user on the same step. Quitting at any
// point preserves `current_step` in SQLite (migration 0011).

import { useCallback, useEffect, useState, type ReactNode } from "react";

import { Button, Logo, PasswordField, Select } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type Account,
  type AiProvider,
  type AiSettings,
  type BusinessContext,
  type Environment,
  type EnvironmentType,
  type OnboardingState,
  type OnboardingStep,
  type ProfileInfo,
  type ProvisioningStatus,
  type RiskTolerance,
  type ScanRecord,
  type TeamSize,
} from "@/lib/ipc";
import ConnectScannerRoleForm from "@/components/ConnectScannerRoleForm";
import { SCAN_FINISHED_EVENT, useScanModal } from "@/contexts/ScanModalContext";
import { type Locale, LOCALES } from "@/lib/i18n";
import { useLock } from "@/stores/lock";
import { useLocale } from "@/stores/locale";

/** Routes the onboarding wizard knows how to redirect to on finish.
 *  Currently only the first-scan step uses this — when the user's
 *  bootstrap scan completes, FirstScanStep requests a redirect to
 *  Findings so they see their results immediately. */
export type OnboardingLandingRoute = "findings";

type Props = {
  /** Optional callback the wizard fires after `onboardingComplete()`
   * succeeds, so App.tsx can re-hydrate and route to the main app.
   * The optional `landingRoute` argument lets a step request a
   * specific post-onboarding destination — App.tsx flips the route
   * BEFORE re-hydrating onboarding state so the user lands directly
   * on that page rather than transiently seeing the default Dashboard
   * Welcome surface (PR #52). */
  onCompleted?: (landingRoute?: OnboardingLandingRoute) => void;
};

const STEP_INDEX: Record<OnboardingStep, number> = {
  language: 1,
  master_password: 2,
  aws_account: 3,
  terraform: 4,
  business_context: 5,
  first_scan: 6,
  done: 7,
};

const STEPS: OnboardingStep[] = [
  "language",
  "master_password",
  "aws_account",
  "terraform",
  "business_context",
  "first_scan",
];

function nextStep(step: OnboardingStep): OnboardingStep {
  const i = STEPS.indexOf(step);
  if (i === -1 || i === STEPS.length - 1) return "done";
  return STEPS[i + 1];
}
function prevStep(step: OnboardingStep): OnboardingStep {
  const i = STEPS.indexOf(step);
  if (i <= 0) return STEPS[0];
  return STEPS[i - 1];
}

export default function Onboarding({ onCompleted }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [state, setState] = useState<OnboardingState | null>(null);
  const [topErr, setTopErr] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setTopErr(null);
    try {
      setState(await ipc.onboardingGetState());
    } catch (e) {
      setTopErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function advance(from: OnboardingStep) {
    try {
      await ipc.onboardingMarkStepCompleted(from);
      const next = nextStep(from);
      await ipc.onboardingSetCurrentStep(next);
      await reload();
    } catch (e) {
      setTopErr(formatError(e));
    }
  }

  // Skip moves to the next step WITHOUT marking the current step
  // completed — the user can come back via Settings to finish it.
  async function skip(from: OnboardingStep) {
    try {
      await ipc.onboardingSetCurrentStep(nextStep(from));
      await reload();
    } catch (e) {
      setTopErr(formatError(e));
    }
  }

  async function goBack(from: OnboardingStep) {
    try {
      await ipc.onboardingSetCurrentStep(prevStep(from));
      await reload();
    } catch (e) {
      setTopErr(formatError(e));
    }
  }

  async function finish(landingRoute?: OnboardingLandingRoute) {
    try {
      await ipc.onboardingComplete();
      onCompleted?.(landingRoute);
    } catch (e) {
      setTopErr(formatError(e));
    }
  }

  if (!state) {
    return (
      <main className="min-h-full bg-saw-grey-50 flex items-center justify-center">
        <p className="text-body text-saw-grey-600">{t("common.loading")}</p>
      </main>
    );
  }

  const step = state.current_step;
  const stepIdx = STEP_INDEX[step];
  const total = STEPS.length;

  return (
    <main className="min-h-full bg-saw-grey-50 px-6 py-10">
      <div className="mx-auto max-w-2xl">
        <header className="mb-6">
          <div className="flex items-center gap-3">
            <Logo size="md" />
            <div>
              <h1 className="text-h1 font-semibold text-saw-grey-900">
                {t("onboarding.title")}
              </h1>
              <p className="mt-1 text-small text-saw-grey-600">
                {t("onboarding.subtitle")}
              </p>
            </div>
          </div>
          <ProgressBar current={Math.min(stepIdx, total)} total={total} />
        </header>

        {topErr ? (
          <p
            role="alert"
            className="mb-4 rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
            data-testid="onboarding-top-error"
          >
            {topErr}
          </p>
        ) : null}

        {step === "language" ? (
          <LanguageStep
            state={state}
            onContinue={() => advance("language")}
            onSkip={() => void skip("language")}
          />
        ) : step === "master_password" ? (
          <PasswordStep
            onBack={() => goBack(step)}
            onContinue={() => advance("master_password")}
            onSkip={() => void skip("master_password")}
          />
        ) : step === "aws_account" ? (
          <AwsAccountStep
            onBack={() => goBack(step)}
            onContinue={() => advance("aws_account")}
            onSkip={() => void skip("aws_account")}
          />
        ) : step === "terraform" ? (
          <ScannerRoleStep
            onBack={() => goBack(step)}
            onContinue={() => advance("terraform")}
            onSkip={() => void skip("terraform")}
          />
        ) : step === "business_context" ? (
          <BusinessContextStep
            onBack={() => goBack(step)}
            onContinue={() => advance("business_context")}
            onSkip={() => void skip("business_context")}
          />
        ) : step === "first_scan" ? (
          <FirstScanStep
            onBack={() => goBack(step)}
            onContinue={() => {
              void advance("first_scan");
            }}
            onSkip={() => void skip("first_scan")}
            onFinish={(landingRoute) => void finish(landingRoute)}
          />
        ) : (
          <DoneCard onFinish={() => void finish()} />
        )}
      </div>
    </main>
  );
}

// --- Components ---------------------------------------------------------

function ProgressBar({ current, total }: { current: number; total: number }) {
  const t = useT();
  return (
    <div className="mt-4">
      <div
        className="text-xs text-saw-grey-600"
        data-testid="onboarding-progress-label"
      >
        {t("onboarding.progress")
          .replace("{current}", String(current))
          .replace("{total}", String(total))}
      </div>
      <div
        className="mt-1 h-2 rounded-full bg-saw-grey-200 overflow-hidden"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={total}
        aria-valuenow={current}
      >
        <div
          className="h-full bg-saw-red transition-all"
          style={{ width: `${Math.min(100, (current / total) * 100)}%` }}
          data-testid="onboarding-progress-bar"
        />
      </div>
    </div>
  );
}

function StepCard({
  title,
  body,
  children,
  testId,
}: {
  title: string;
  body: string;
  children: ReactNode;
  testId: string;
}) {
  return (
    <section
      className="rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid={testId}
    >
      <h2 className="text-h2 font-semibold text-saw-grey-900">{title}</h2>
      <p className="mt-2 text-body text-saw-grey-700">{body}</p>
      <div className="mt-5">{children}</div>
    </section>
  );
}

function CliBlock({
  lines,
  warning,
}: {
  lines: string[];
  warning?: string;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  function copy() {
    if (!navigator.clipboard) return;
    void navigator.clipboard.writeText(lines.join("\n")).then(
      () => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 2000);
      },
      () => undefined,
    );
  }
  return (
    <div className="mt-3 rounded-card border border-saw-grey-200 bg-saw-grey-50 p-3 text-small">
      <div className="flex items-center justify-between">
        <span className="text-saw-grey-700">{t("onboarding.cli_label")}</span>
        <button
          type="button"
          onClick={copy}
          className="text-xs underline underline-offset-2"
          data-testid="onboarding-cli-copy"
        >
          {copied ? t("onboarding.cli_copied") : t("onboarding.cli_copy")}
        </button>
      </div>
      <pre
        className="mt-2 overflow-auto rounded bg-saw-white p-2 font-mono text-xs text-saw-grey-900"
        data-testid="onboarding-cli-block"
      >
        {lines.join("\n")}
      </pre>
      <p className="mt-2 text-xs text-saw-red">
        {warning ?? t("onboarding.cli_warning")}
      </p>
    </div>
  );
}

function ManualToggle({
  showing,
  onToggle,
}: {
  showing: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={onToggle}
      data-testid="onboarding-manual-toggle"
    >
      {showing ? t("onboarding.nav.do_myself_close") : t("onboarding.nav.do_myself")}
    </Button>
  );
}

// --- Step 1: Language ---------------------------------------------------

function LanguageStep({
  state,
  onContinue,
  onSkip,
}: {
  state: OnboardingState;
  onContinue: () => void;
  onSkip: () => void;
}) {
  const t = useT();
  const { locale, setLocale } = useLocale();
  const formatError = useIpcError();
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Apply the persisted language as soon as the step mounts so the
  // wizard reflects the user's prior pick if they're resuming.
  useEffect(() => {
    if (LOCALES.includes(state.language as Locale)) {
      setLocale(state.language as Locale);
    }
  }, [state.language, setLocale]);

  async function pick(next: Locale) {
    setLocale(next);
    setSaving(true);
    setErr(null);
    try {
      await ipc.onboardingSetLanguage(next);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <StepCard
      testId="onboarding-step-language"
      title={t("onboarding.step.language.title")}
      body={t("onboarding.step.language.body")}
    >
      <Select<Locale>
        label={t("onboarding.step.language.label")}
        value={locale}
        options={[
          { value: "en", label: t("onboarding.step.language.en") },
          { value: "es", label: t("onboarding.step.language.es") },
          { value: "fr", label: t("onboarding.step.language.fr") },
          { value: "zh", label: t("onboarding.step.language.zh") },
        ]}
        onChange={(v) => void pick(v)}
        data-testid="onboarding-language-select"
      />
      {err ? (
        <p role="alert" className="mt-2 text-small text-saw-red">
          {err}
        </p>
      ) : null}
      <div className="mt-4 flex justify-end gap-2">
        <Button
          variant="ghost"
          onClick={onSkip}
          data-testid="onboarding-language-skip"
        >
          {t("onboarding.nav.skip")}
        </Button>
        <Button
          variant="primary"
          onClick={onContinue}
          disabled={saving}
          data-testid="onboarding-language-continue"
        >
          {t("onboarding.step.language.cta")}
        </Button>
      </div>
    </StepCard>
  );
}

// --- Step 2: Master password -------------------------------------------

const MIN_PASSWORD_LEN = 8;

function PasswordStep({
  onBack,
  onContinue,
  onSkip,
}: {
  onBack: () => void;
  onContinue: () => void;
  onSkip: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const { state: lockState, refresh } = useLock();
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const alreadySet = !lockState?.first_run;
  const tooShort = password.length > 0 && password.length < MIN_PASSWORD_LEN;
  const mismatch =
    password.length > 0 && confirm.length > 0 && password !== confirm;
  const canSubmit =
    !submitting &&
    password.length >= MIN_PASSWORD_LEN &&
    password === confirm;

  async function onSubmit() {
    if (!canSubmit) return;
    setSubmitting(true);
    setErr(null);
    try {
      await ipc.applockSetMasterPassword(password);
      // Drop the password from React state ASAP — the Rust side
      // already zeroized its own copy.
      setPassword("");
      setConfirm("");
      await refresh();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <StepCard
      testId="onboarding-step-password"
      title={t("onboarding.step.password.title")}
      body={t("onboarding.step.password.body")}
    >
      <p className="text-small text-saw-grey-700 rounded-card bg-saw-grey-100 px-3 py-2">
        {t("applock.disclosure")}
      </p>
      {alreadySet ? (
        <p
          className="mt-3 rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-password-already-set"
        >
          {t("onboarding.step.password.already_set")}
        </p>
      ) : (
        <form
          className="mt-4 flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            void onSubmit();
          }}
        >
          <PasswordField
            label={t("applock.field.new_password")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="new-password"
            hint={t("applock.setup.password_hint").replace(
              "{min}",
              String(MIN_PASSWORD_LEN),
            )}
            error={tooShort ? t("applock.error.too_short") : null}
            showLabel={t("applock.field.show")}
            hideLabel={t("applock.field.hide")}
            data-testid="onboarding-password"
          />
          <PasswordField
            label={t("applock.field.confirm_password")}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            autoComplete="new-password"
            error={mismatch ? t("applock.error.mismatch") : null}
            showLabel={t("applock.field.show")}
            hideLabel={t("applock.field.hide")}
            data-testid="onboarding-password-confirm"
          />
          <div>
            <Button
              type="submit"
              variant="primary"
              disabled={!canSubmit}
              data-testid="onboarding-password-submit"
            >
              {submitting
                ? t("applock.setup.submitting")
                : t("onboarding.step.password.cta")}
            </Button>
          </div>
        </form>
      )}

      {err ? (
        <p role="alert" className="mt-3 text-small text-saw-red">
          {err}
        </p>
      ) : null}

      <div className="mt-5 flex items-center justify-between">
        <Button variant="ghost" onClick={onBack} data-testid="onboarding-password-back">
          {t("onboarding.nav.back")}
        </Button>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            onClick={onSkip}
            data-testid="onboarding-password-skip"
          >
            {t("onboarding.nav.skip")}
          </Button>
          <Button
            variant="primary"
            onClick={onContinue}
            disabled={!alreadySet}
            data-testid="onboarding-password-next"
          >
            {t("onboarding.nav.next")}
          </Button>
        </div>
      </div>
    </StepCard>
  );
}

// --- Step 3: AWS account ------------------------------------------------

function AwsAccountStep({
  onBack,
  onContinue,
  onSkip,
}: {
  onBack: () => void;
  onContinue: () => void;
  onSkip: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [profilesLoaded, setProfilesLoaded] = useState(false);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [profileName, setProfileName] = useState("");
  const [environment, setEnvironment] = useState<Environment>("dev");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [showManual, setShowManual] = useState(false);

  const reload = useCallback(async () => {
    setErr(null);
    try {
      const [p, accs, act] = await Promise.all([
        ipc.authListProfiles(),
        ipc.accountsList(),
        ipc.accountsGetActive(),
      ]);
      setProfiles(p);
      setProfilesLoaded(true);
      setAccounts(accs);
      setActive(act);
      if (!profileName && p.length > 0) setProfileName(p[0].name);
    } catch (e) {
      setErr(formatError(e));
      setProfilesLoaded(true);
    }
  }, [formatError, profileName]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function addAccount() {
    if (!label.trim() || !profileName) return;
    setBusy(true);
    setErr(null);
    try {
      await ipc.accountsAdd({
        label: label.trim(),
        profile_name: profileName,
        environment,
      });
      setLabel("");
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  const hasAccount = accounts.length > 0;
  const noProfiles = profilesLoaded && profiles.length === 0;

  return (
    <StepCard
      testId="onboarding-step-account"
      title={t("onboarding.step.account.title")}
      body={t("onboarding.step.account.body")}
    >
      {noProfiles ? (
        <div
          className="rounded-card border border-saw-red/40 bg-saw-red/5 px-3 py-2 text-small"
          data-testid="onboarding-account-no-cli"
        >
          <div className="font-medium text-saw-red">
            {t("onboarding.step.account.no_cli_title")}
          </div>
          <p className="mt-1 text-saw-grey-800">
            {t("onboarding.step.account.no_cli_body")}
          </p>
          <ul className="mt-2 list-disc pl-5 text-xs text-saw-grey-700">
            <li>{t("onboarding.step.account.no_cli_windows")}</li>
            <li>{t("onboarding.step.account.no_cli_macos")}</li>
            <li>{t("onboarding.step.account.no_cli_linux")}</li>
          </ul>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1 text-small text-saw-grey-700">
            <span>{t("accounts.add.label_field")}</span>
            <input
              type="text"
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder="acme-dev"
              className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
              data-testid="onboarding-account-label"
            />
          </label>
          <label className="flex flex-col gap-1 text-small text-saw-grey-700">
            <span>{t("accounts.add.profile_field")}</span>
            <select
              value={profileName}
              onChange={(e) => setProfileName(e.target.value)}
              className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
              data-testid="onboarding-account-profile"
            >
              {profiles.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name} ({t(`profiles.source.${p.source}`)})
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-small text-saw-grey-700">
            <span>{t("accounts.add.environment_field")}</span>
            <select
              value={environment}
              onChange={(e) => setEnvironment(e.target.value as Environment)}
              className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
              data-testid="onboarding-account-env"
            >
              <option value="dev">{t("accounts.env.dev")}</option>
              <option value="staging">{t("accounts.env.staging")}</option>
              <option value="prod">{t("accounts.env.prod")}</option>
              <option value="other">{t("accounts.env.other")}</option>
            </select>
          </label>
          <div className="flex gap-2">
            <Button
              variant="primary"
              onClick={() => void addAccount()}
              disabled={busy || !label.trim() || !profileName}
              data-testid="onboarding-account-add"
            >
              {busy ? t("accounts.add.verifying") : t("onboarding.step.account.add_cta")}
            </Button>
          </div>
        </div>
      )}

      <div className="mt-4">
        <ManualToggle showing={showManual} onToggle={() => setShowManual(!showManual)} />
        {showManual ? (
          <CliBlock
            lines={[
              "# Configure a vanilla profile",
              "aws configure",
              "",
              "# Or, an IAM Identity Center (SSO) profile",
              "aws configure sso",
              "",
              "# Verify it resolves:",
              "aws sts get-caller-identity --profile <your-profile>",
            ]}
          />
        ) : null}
      </div>

      {hasAccount ? (
        <p
          className="mt-3 rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-account-added"
        >
          {t("onboarding.step.account.added")}{" "}
          {accounts.map((a) => a.label).join(", ")}
          {active ? ` (active: ${accounts.find((a) => a.aws_account_id === active)?.label ?? active})` : null}
        </p>
      ) : null}

      {err ? (
        <p role="alert" className="mt-3 text-small text-saw-red">
          {err}
        </p>
      ) : null}

      <div className="mt-5 flex items-center justify-between">
        <Button variant="ghost" onClick={onBack} data-testid="onboarding-account-back">
          {t("onboarding.nav.back")}
        </Button>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            onClick={onSkip}
            data-testid="onboarding-account-skip"
          >
            {t("onboarding.nav.skip")}
          </Button>
          <Button
            variant="primary"
            onClick={onContinue}
            disabled={!hasAccount}
            data-testid="onboarding-account-continue"
          >
            {t("onboarding.nav.next")}
          </Button>
        </div>
      </div>
    </StepCard>
  );
}

// --- Step 4: Scanner role (Phase 2 — replaces deleted Terraform step) ---

function ScannerRoleStep({
  onBack,
  onContinue,
  onSkip,
}: {
  onBack: () => void;
  onContinue: () => void;
  onSkip: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [status, setStatus] = useState<ProvisioningStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setErr(null);
    try {
      const [accs, act, profs] = await Promise.all([
        ipc.accountsList(),
        ipc.accountsGetActive(),
        ipc.authListProfiles(),
      ]);
      setAccounts(accs);
      setActive(act);
      setProfiles(profs);
      if (act) {
        const s = await ipc.scannerRoleStatus(act);
        setStatus(s);
      }
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const activeAccount = accounts.find((a) => a.aws_account_id === active) ?? null;
  const profileMissing =
    activeAccount && !profiles.some((p) => p.name === activeAccount.profile_name);
  const provisioned = status?.status === "provisioned";

  return (
    <StepCard
      testId="onboarding-step-scanner-role"
      title={t("onboarding.step.terraform.title")}
      body={t("onboarding.step.terraform.body")}
    >
      {!activeAccount ? (
        <p
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-scanner-role-no-account"
        >
          {t("onboarding.step.terraform.no_account_hint")}
        </p>
      ) : profileMissing ? (
        <p
          className="rounded-card border border-saw-red/40 bg-saw-red/5 px-3 py-2 text-small text-saw-grey-900"
          data-testid="onboarding-scanner-role-profile-missing"
        >
          {t("onboarding.step.terraform.profile_missing_hint")}
        </p>
      ) : provisioned ? (
        <p
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-scanner-role-completed"
        >
          {t("onboarding.step.terraform.completed")}
        </p>
      ) : (
        <ConnectScannerRoleForm
          awsAccountId={activeAccount.aws_account_id}
          onConnected={() => {
            void reload();
          }}
        />
      )}

      {err ? (
        <p role="alert" className="mt-3 text-small text-saw-red">
          {err}
        </p>
      ) : null}

      <div className="mt-5 flex items-center justify-between">
        <Button variant="ghost" onClick={onBack} data-testid="onboarding-scanner-role-back">
          {t("onboarding.nav.back")}
        </Button>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            onClick={onSkip}
            data-testid="onboarding-scanner-role-skip"
          >
            {t("onboarding.nav.skip")}
          </Button>
          <Button
            variant="primary"
            onClick={onContinue}
            disabled={!provisioned}
            data-testid="onboarding-scanner-role-continue"
          >
            {t("onboarding.nav.next")}
          </Button>
        </div>
      </div>
    </StepCard>
  );
}

// --- Step 5: Business context (optional) -------------------------------

function BusinessContextStep({
  onBack,
  onContinue,
  onSkip,
}: {
  onBack: () => void;
  onContinue: () => void;
  onSkip: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [provider, setProvider] = useState<AiProvider | "">("");
  const [keyInput, setKeyInput] = useState("");
  const [context, setContext] = useState<BusinessContext | null>(null);
  const [complianceInput, setComplianceInput] = useState("");
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const s = await ipc.aiGetSettings();
      setSettings(s);
      setProvider(s.provider ?? "");
      setContext(s.context);
      setComplianceInput(s.context.compliance.join(", "));
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function saveAndContinue() {
    if (!context) return;
    setSaving(true);
    setErr(null);
    try {
      // Persist provider selection (or clear it).
      await ipc.aiSetProvider(provider === "" ? null : provider);
      // If a fresh key was typed, hand it to the OS keychain. Never
      // store the in-memory string anywhere else — drop it as soon
      // as the IPC returns.
      if (provider !== "" && keyInput.trim().length > 0) {
        await ipc.aiSetProviderKey(provider, keyInput.trim());
        setKeyInput("");
      }
      const compliance = complianceInput
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s.length > 0);
      await ipc.aiSetBusinessContext({ ...context, compliance });
      onContinue();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setSaving(false);
    }
  }

  async function clearKey() {
    if (provider === "") return;
    setErr(null);
    try {
      await ipc.aiClearProviderKey(provider);
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

  if (!settings || !context) {
    return (
      <StepCard
        testId="onboarding-step-context"
        title={t("onboarding.step.context.title")}
        body={t("onboarding.step.context.body")}
      >
        <p className="text-small text-saw-grey-600">{t("common.loading")}</p>
      </StepCard>
    );
  }

  return (
    <StepCard
      testId="onboarding-step-context"
      title={t("onboarding.step.context.title")}
      body={t("onboarding.step.context.body")}
    >
      <div className="rounded-card border border-saw-red/30 bg-saw-red/5 p-3 text-small">
        <div className="font-medium text-saw-red">{t("ai.disclosure.title")}</div>
        <div className="mt-1 text-saw-grey-800">{t("ai.disclosure.body")}</div>
      </div>

      <div className="mt-4 flex flex-col gap-4">
        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.provider.label")}</span>
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value as AiProvider | "")}
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="onboarding-ai-provider"
          >
            <option value="">{t("ai.provider.none")}</option>
            <option value="anthropic">{t("ai.provider.anthropic")}</option>
            <option value="openai">{t("ai.provider.openai")}</option>
          </select>
        </label>

        {provider !== "" ? (
          <label className="flex flex-col gap-1 text-small text-saw-grey-700">
            <span>{t("ai.key.label")}</span>
            <input
              type="password"
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              placeholder={
                provider === "anthropic"
                  ? t("ai.key.placeholder_anthropic")
                  : t("ai.key.placeholder_openai")
              }
              autoComplete="off"
              className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900 font-mono"
              data-testid="onboarding-ai-key"
            />
            <span className="text-xs text-saw-grey-500">{t("ai.key.hint")}</span>
            <p
              className="text-small text-saw-grey-700"
              data-testid="onboarding-ai-key-status"
            >
              {settings.key_connected
                ? t("ai.key.connected")
                : t("ai.key.not_connected")}
            </p>
            {settings.key_connected ? (
              <div>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void clearKey()}
                  data-testid="onboarding-ai-key-clear"
                >
                  {t("ai.key.clear")}
                </Button>
              </div>
            ) : null}
          </label>
        ) : null}

        <hr className="border-saw-grey-100" />

        <div>
          <div className="font-medium text-saw-grey-900">
            {t("ai.context.title")}
          </div>
          <div className="mt-1 text-small text-saw-grey-600">
            {t("ai.context.subtitle")}
          </div>
        </div>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.industry")}</span>
          <input
            type="text"
            value={context.industry}
            onChange={(e) => setContext({ ...context, industry: e.target.value })}
            placeholder={t("ai.context.industry_placeholder")}
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="onboarding-ctx-industry"
          />
          {settings.flags.industry_identifying ? (
            <span
              className="text-xs text-saw-red"
              data-testid="onboarding-ctx-industry-warn"
            >
              {t("ai.context.industry_warn")}
            </span>
          ) : null}
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.environment")}</span>
          <select
            value={context.environment_type}
            onChange={(e) =>
              setContext({
                ...context,
                environment_type: e.target.value as EnvironmentType,
              })
            }
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="onboarding-ctx-env"
          >
            {envOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.compliance")}</span>
          <input
            type="text"
            value={complianceInput}
            onChange={(e) => setComplianceInput(e.target.value)}
            placeholder={t("ai.context.compliance_placeholder")}
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900 font-mono"
            data-testid="onboarding-ctx-compliance"
          />
          {settings.flags.compliance_identifying ? (
            <span
              className="text-xs text-saw-red"
              data-testid="onboarding-ctx-compliance-warn"
            >
              {t("ai.context.compliance_warn")}
            </span>
          ) : null}
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.risk")}</span>
          <select
            value={context.risk_tolerance}
            onChange={(e) =>
              setContext({
                ...context,
                risk_tolerance: e.target.value as RiskTolerance,
              })
            }
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="onboarding-ctx-risk"
          >
            {riskOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.team")}</span>
          <select
            value={context.team_size}
            onChange={(e) =>
              setContext({
                ...context,
                team_size: e.target.value as TeamSize,
              })
            }
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="onboarding-ctx-team"
          >
            {teamOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      {err ? (
        <p role="alert" className="mt-3 text-small text-saw-red">
          {err}
        </p>
      ) : null}

      <div className="mt-5 flex items-center justify-between">
        <Button variant="ghost" onClick={onBack} data-testid="onboarding-context-back">
          {t("onboarding.nav.back")}
        </Button>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            onClick={onSkip}
            disabled={saving}
            data-testid="onboarding-context-skip"
          >
            {t("onboarding.nav.skip")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void saveAndContinue()}
            disabled={saving}
            data-testid="onboarding-context-continue"
          >
            {saving
              ? t("ai.key.saving")
              : t("onboarding.step.context.continue_cta")}
          </Button>
        </div>
      </div>
    </StepCard>
  );
}

// --- Step 6: First scan -------------------------------------------------

function FirstScanStep({
  onBack,
  onContinue,
  onSkip,
  onFinish,
}: {
  onBack: () => void;
  onContinue: () => void;
  onSkip: () => void;
  /** PR #52: now accepts an optional landing route. The first-
   *  scan step requests "findings" so the user lands on the
   *  Findings page immediately after a scan completes. */
  onFinish: (landingRoute?: OnboardingLandingRoute) => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const { open: openScanModal } = useScanModal();
  const [active, setActive] = useState<string | null>(null);
  const [account, setAccount] = useState<Account | null>(null);
  const [recent, setRecent] = useState<ScanRecord[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [showManual, setShowManual] = useState(false);

  const reload = useCallback(async () => {
    setErr(null);
    try {
      const act = await ipc.accountsGetActive();
      setActive(act);
      if (act) {
        // Need the full Account object for `useScanModal().open({ account })`
        // to pre-bind the modal — fetch via accountsList (single small call).
        const [list, scanList] = await Promise.all([
          ipc.accountsList(),
          ipc.scannerListRecent(act, 5),
        ]);
        const found = list.find((a) => a.aws_account_id === act);
        setAccount(found ?? null);
        setRecent(scanList);
      } else {
        setAccount(null);
        setRecent([]);
      }
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
    // Keep the 3s poll: covers the legacy path where a user runs a
    // scan externally (e.g. via the embedded Accounts panel) and
    // comes back to onboarding. The new Scan Now button is the
    // primary affordance.
    const id = window.setInterval(() => {
      void reload();
    }, 3000);
    return () => window.clearInterval(id);
  }, [reload]);

  // PR #52: when a scan completes anywhere in the app (via the global
  // ScanModalProvider's SCAN_FINISHED_EVENT), finish onboarding and
  // request a redirect to Findings so the user sees their results.
  useEffect(() => {
    const handler = () => {
      onFinish("findings");
    };
    document.addEventListener(SCAN_FINISHED_EVENT, handler);
    return () => document.removeEventListener(SCAN_FINISHED_EVENT, handler);
  }, [onFinish]);

  const hasTerminalScan = (recent ?? []).some(
    (s) =>
      s.status === "complete" ||
      s.status === "complete_with_warnings" ||
      s.status === "failed" ||
      s.status === "canceled",
  );

  const canScan =
    !!account && account.role_provisioned;

  return (
    <StepCard
      testId="onboarding-step-scan"
      title={t("onboarding.step.scan.title")}
      body={t("onboarding.step.scan.body")}
    >
      {!active ? (
        <p className="text-small text-saw-grey-700">
          {t("onboarding.step.terraform.no_account_hint")}
        </p>
      ) : hasTerminalScan ? (
        <p
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-scan-completed-hint"
        >
          {t("onboarding.step.scan.completed_hint")}
        </p>
      ) : !canScan ? (
        <p
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-scan-role-missing-hint"
        >
          {t("onboarding.step.scan.role_missing_hint")}
        </p>
      ) : (
        <p
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-scan-ready-hint"
        >
          {t("onboarding.step.scan.ready_hint")}
        </p>
      )}

      {/* PR #52: primary Scan Now affordance. Opens the global
          ScanModal with the just-configured account pre-bound, so
          the user can actually run their first scan from inside
          the wizard. On terminal scan-complete, the SCAN_FINISHED
          event handler above calls onFinish("findings") to land
          the user on the new Findings page. */}
      {canScan ? (
        <div className="mt-4">
          <Button
            variant="primary"
            onClick={() => account && openScanModal({ account })}
            data-testid="onboarding-scan-now"
          >
            {t("onboarding.step.scan.scan_now")}
          </Button>
        </div>
      ) : null}

      <div className="mt-4">
        <ManualToggle showing={showManual} onToggle={() => setShowManual(!showManual)} />
        {showManual ? (
          <CliBlock
            lines={[
              "# CloudSaw assumes the scanner role and runs ScoutSuite. Equivalent:",
              "aws sts assume-role \\",
              "  --role-arn <CloudSawScanner role ARN> \\",
              "  --role-session-name cloudsaw-manual",
              "scout aws --profile <profile-from-assumed-creds>",
            ]}
          />
        ) : null}
      </div>

      {err ? (
        <p role="alert" className="mt-3 text-small text-saw-red">
          {err}
        </p>
      ) : null}

      <div className="mt-5 flex items-center justify-between">
        <Button variant="ghost" onClick={onBack} data-testid="onboarding-scan-back">
          {t("onboarding.nav.back")}
        </Button>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            onClick={onSkip}
            data-testid="onboarding-scan-skip"
          >
            {t("onboarding.nav.skip")}
          </Button>
          {hasTerminalScan ? (
            <Button
              variant="primary"
              onClick={() => {
                onContinue();
                onFinish("findings");
              }}
              data-testid="onboarding-scan-finish"
            >
              {t("onboarding.step.scan.completed_cta")}
            </Button>
          ) : (
            <Button
              variant="primary"
              onClick={onContinue}
              disabled={!hasTerminalScan}
              data-testid="onboarding-scan-continue"
            >
              {t("onboarding.nav.next")}
            </Button>
          )}
        </div>
      </div>
    </StepCard>
  );
}

function DoneCard({ onFinish }: { onFinish: () => void }) {
  const t = useT();
  return (
    <StepCard
      testId="onboarding-step-done"
      title={t("onboarding.done.title")}
      body={t("onboarding.done.body")}
    >
      <Button variant="primary" onClick={onFinish} data-testid="onboarding-done-cta">
        {t("onboarding.done.cta")}
      </Button>
    </StepCard>
  );
}
