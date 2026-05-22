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

import { Button, PasswordField, Select } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type Account,
  type Environment,
  type OnboardingState,
  type OnboardingStep,
  type ProfileInfo,
  type ProvisioningStatus,
  type ScanRecord,
} from "@/lib/ipc";
import { type Locale, LOCALES } from "@/lib/i18n";
import { useLock } from "@/stores/lock";
import { useLocale } from "@/stores/locale";

type Props = {
  /** Optional callback the wizard fires after `onboardingComplete()`
   * succeeds, so App.tsx can re-hydrate and route to the main app. */
  onCompleted?: () => void;
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

  async function goBack(from: OnboardingStep) {
    try {
      await ipc.onboardingSetCurrentStep(prevStep(from));
      await reload();
    } catch (e) {
      setTopErr(formatError(e));
    }
  }

  async function finish() {
    try {
      await ipc.onboardingComplete();
      onCompleted?.();
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
          <h1 className="text-h1 font-semibold text-saw-grey-900">
            {t("onboarding.title")}
          </h1>
          <p className="mt-1 text-small text-saw-grey-600">
            {t("onboarding.subtitle")}
          </p>
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
          <LanguageStep state={state} onContinue={() => advance("language")} />
        ) : step === "master_password" ? (
          <PasswordStep
            onBack={() => goBack(step)}
            onContinue={() => advance("master_password")}
          />
        ) : step === "aws_account" ? (
          <AwsAccountStep
            onBack={() => goBack(step)}
            onContinue={() => advance("aws_account")}
          />
        ) : step === "terraform" ? (
          <TerraformStep
            onBack={() => goBack(step)}
            onContinue={() => advance("terraform")}
          />
        ) : step === "business_context" ? (
          <BusinessContextStep
            onBack={() => goBack(step)}
            onContinue={() => advance("business_context")}
          />
        ) : step === "first_scan" ? (
          <FirstScanStep
            onBack={() => goBack(step)}
            onContinue={() => {
              void advance("first_scan");
            }}
            onFinish={() => void finish()}
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
}: {
  state: OnboardingState;
  onContinue: () => void;
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
}: {
  onBack: () => void;
  onContinue: () => void;
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
        <Button
          variant="primary"
          onClick={onContinue}
          disabled={!alreadySet}
          data-testid="onboarding-password-next"
        >
          {t("onboarding.nav.next")}
        </Button>
      </div>
    </StepCard>
  );
}

// --- Step 3: AWS account ------------------------------------------------

function AwsAccountStep({
  onBack,
  onContinue,
}: {
  onBack: () => void;
  onContinue: () => void;
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
        <Button
          variant="primary"
          onClick={onContinue}
          disabled={!hasAccount}
          data-testid="onboarding-account-continue"
        >
          {t("onboarding.nav.next")}
        </Button>
      </div>
    </StepCard>
  );
}

// --- Step 4: Terraform --------------------------------------------------

function TerraformStep({
  onBack,
  onContinue,
}: {
  onBack: () => void;
  onContinue: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [status, setStatus] = useState<ProvisioningStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [showManual, setShowManual] = useState(false);

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
        const s = await ipc.terraformProvisioningStatus(act);
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
      testId="onboarding-step-terraform"
      title={t("onboarding.step.terraform.title")}
      body={t("onboarding.step.terraform.body")}
    >
      {!activeAccount ? (
        <p
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-terraform-no-account"
        >
          {t("onboarding.step.terraform.no_account_hint")}
        </p>
      ) : profileMissing ? (
        <p
          className="rounded-card border border-saw-red/40 bg-saw-red/5 px-3 py-2 text-small text-saw-grey-900"
          data-testid="onboarding-terraform-profile-missing"
        >
          {t("onboarding.step.terraform.profile_missing_hint")}
        </p>
      ) : provisioned ? (
        <p
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="onboarding-terraform-already-provisioned"
        >
          {t("onboarding.step.terraform.completed")}
        </p>
      ) : (
        <p className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700">
          {t("terraform.provision.subtitle").replace("{role_name}", "CloudSawScanner")}
        </p>
      )}

      <div className="mt-4">
        <ManualToggle showing={showManual} onToggle={() => setShowManual(!showManual)} />
        {showManual ? (
          <CliBlock
            lines={[
              "# CloudSaw runs the equivalent of:",
              "terraform -chdir=<workdir> init",
              "terraform -chdir=<workdir> plan -out=cloudsaw.tfplan",
              "# Review the plan before applying:",
              "terraform -chdir=<workdir> show cloudsaw.tfplan",
              "terraform -chdir=<workdir> apply cloudsaw.tfplan",
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
        <Button variant="ghost" onClick={onBack} data-testid="onboarding-terraform-back">
          {t("onboarding.nav.back")}
        </Button>
        <Button
          variant="primary"
          onClick={onContinue}
          disabled={!provisioned}
          data-testid="onboarding-terraform-continue"
        >
          {t("onboarding.nav.next")}
        </Button>
      </div>
    </StepCard>
  );
}

// --- Step 5: Business context (optional) -------------------------------

function BusinessContextStep({
  onBack,
  onContinue,
}: {
  onBack: () => void;
  onContinue: () => void;
}) {
  const t = useT();
  return (
    <StepCard
      testId="onboarding-step-context"
      title={t("onboarding.step.context.title")}
      body={t("onboarding.step.context.body")}
    >
      <p className="text-small text-saw-grey-600">
        {t("ai.section_subtitle")}
      </p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button
          variant="secondary"
          onClick={onContinue}
          data-testid="onboarding-context-skip"
        >
          {t("onboarding.step.context.skip_cta")}
        </Button>
        <Button
          variant="primary"
          onClick={onContinue}
          data-testid="onboarding-context-continue"
        >
          {t("onboarding.step.context.continue_cta")}
        </Button>
      </div>
      <div className="mt-5 flex justify-start">
        <Button variant="ghost" onClick={onBack} data-testid="onboarding-context-back">
          {t("onboarding.nav.back")}
        </Button>
      </div>
    </StepCard>
  );
}

// --- Step 6: First scan -------------------------------------------------

function FirstScanStep({
  onBack,
  onContinue,
  onFinish,
}: {
  onBack: () => void;
  onContinue: () => void;
  onFinish: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [active, setActive] = useState<string | null>(null);
  const [recent, setRecent] = useState<ScanRecord[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [showManual, setShowManual] = useState(false);

  const reload = useCallback(async () => {
    setErr(null);
    try {
      const act = await ipc.accountsGetActive();
      setActive(act);
      if (act) {
        const list = await ipc.scannerListRecent(act, 5);
        setRecent(list);
      } else {
        setRecent([]);
      }
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
    const id = window.setInterval(() => {
      void reload();
    }, 3000);
    return () => window.clearInterval(id);
  }, [reload]);

  const hasTerminalScan = (recent ?? []).some(
    (s) =>
      s.status === "complete" ||
      s.status === "complete_with_warnings" ||
      s.status === "failed" ||
      s.status === "canceled",
  );

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
      ) : (
        <p className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700">
          {t("scanner.scan.subtitle").replace("{label}", "")}
        </p>
      )}

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
        {hasTerminalScan ? (
          <Button
            variant="primary"
            onClick={() => {
              onContinue();
              onFinish();
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
