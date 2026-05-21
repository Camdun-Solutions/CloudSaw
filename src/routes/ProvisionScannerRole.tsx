// Scanner-role provisioning flow (Contract 05).
//
// The flow is a single modal with three stages:
//
//   1. Detect   — terraform_detect on mount; gates the rest of the flow.
//   2. Plan     — user picks policy variant, clicks Generate; terraform_plan
//                 returns a fresh plan_token + diff.
//   3. Apply    — user confirms the diff and clicks Apply; terraform_apply
//                 consumes the plan_token, runs `terraform apply`, and
//                 records the role ARN.
//
// The modal is self-contained — it owns its phase state and reloads
// `provisioning_status` and the account list via `onProvisioned` when the
// apply succeeds. Any IPC failure surfaces as a localized message via
// `useIpcError`; the modal never displays raw backend strings.
//
// Why a separate file vs. inline in Accounts.tsx: Accounts.tsx is already
// large and owns three modals (add/edit/remove). Keeping the provisioning
// flow here means each contract's UI lives in its own file and tests can
// target a stable filename.

import { useCallback, useEffect, useMemo, useState } from "react";

import { Badge, Button, Modal, Select } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type Account,
  type ApplyResult,
  type PlanChange,
  type PlanResult,
  type PolicyVariant,
  type TerraformAvailability,
} from "@/lib/ipc";

type Props = {
  account: Account | null;
  onClose: () => void;
  onProvisioned: () => Promise<void>;
};

type Phase =
  | { kind: "detecting" }
  | { kind: "detect_result"; availability: TerraformAvailability }
  | { kind: "planning"; availability: TerraformAvailability }
  | {
      kind: "plan_ready";
      availability: TerraformAvailability;
      plan: PlanResult;
    }
  | {
      kind: "applying";
      availability: TerraformAvailability;
      plan: PlanResult;
    }
  | { kind: "applied"; result: ApplyResult };

export default function ProvisionScannerRoleModal({
  account,
  onClose,
  onProvisioned,
}: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [phase, setPhase] = useState<Phase>({ kind: "detecting" });
  const [policyVariant, setPolicyVariant] =
    useState<PolicyVariant>("security_audit");
  const [error, setError] = useState<string | null>(null);

  // Re-run detection every time the modal opens for a new account — the
  // bundled binary can't change while the app is running, but the detection
  // result also includes the SHA-256 which is displayed in the UI.
  useEffect(() => {
    if (!account) return;
    let cancelled = false;
    setPhase({ kind: "detecting" });
    setError(null);
    setPolicyVariant("security_audit");
    ipc
      .terraformDetect()
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
    };
  }, [account, formatError]);

  const onGeneratePlan = useCallback(async () => {
    if (!account) return;
    setError(null);
    const availability =
      phase.kind === "detect_result" || phase.kind === "plan_ready"
        ? phase.availability
        : null;
    if (!availability) return;
    setPhase({ kind: "planning", availability });
    try {
      const plan = await ipc.terraformPlan(account.aws_account_id, {
        policy_variant: policyVariant,
      });
      setPhase({ kind: "plan_ready", availability, plan });
    } catch (err) {
      setError(formatError(err));
      setPhase({ kind: "detect_result", availability });
    }
  }, [account, phase, policyVariant, formatError]);

  const onApply = useCallback(async () => {
    if (!account) return;
    if (phase.kind !== "plan_ready") return;
    setError(null);
    const { availability, plan } = phase;
    setPhase({ kind: "applying", availability, plan });
    try {
      const result = await ipc.terraformApply(
        account.aws_account_id,
        plan.plan_token,
      );
      setPhase({ kind: "applied", result });
      await onProvisioned();
    } catch (err) {
      setError(formatError(err));
      setPhase({ kind: "plan_ready", availability, plan });
    }
  }, [account, phase, formatError, onProvisioned]);

  const onBackToOptions = useCallback(() => {
    if (phase.kind === "plan_ready") {
      setError(null);
      setPhase({ kind: "detect_result", availability: phase.availability });
    }
  }, [phase]);

  if (!account) return null;

  const title = t("terraform.provision.title");

  return (
    <Modal
      open={true}
      onClose={onClose}
      title={title}
      footer={renderFooter({
        phase,
        policyVariant,
        t,
        onClose,
        onGeneratePlan,
        onApply,
        onBackToOptions,
      })}
    >
      <div className="flex flex-col gap-4">
        <p className="text-small text-saw-grey-600">
          {t("terraform.provision.subtitle").replace(
            "{role_name}",
            "CloudSawScannerRole",
          )}
        </p>

        <AccountSummary account={account} />

        <DetectionSection phase={phase} />

        {(phase.kind === "detect_result" ||
          phase.kind === "planning") &&
        phase.availability.status === "available" ? (
          <PolicySection
            value={policyVariant}
            onChange={setPolicyVariant}
            disabled={phase.kind === "planning"}
          />
        ) : null}

        {phase.kind === "plan_ready" ? (
          <PlanSection plan={phase.plan} />
        ) : null}

        {phase.kind === "applied" ? (
          <AppliedSection result={phase.result} />
        ) : null}

        {error ? (
          <p
            role="alert"
            className="rounded-card border border-saw-grey-200 bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
            data-testid="terraform-error"
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
  policyVariant,
  t,
  onClose,
  onGeneratePlan,
  onApply,
  onBackToOptions,
}: {
  phase: Phase;
  policyVariant: PolicyVariant;
  t: (k: string) => string;
  onClose: () => void;
  onGeneratePlan: () => void;
  onApply: () => void;
  onBackToOptions: () => void;
}) {
  switch (phase.kind) {
    case "detecting":
      return (
        <Button variant="ghost" onClick={onClose} data-testid="terraform-cancel">
          {t("terraform.provision.cancel")}
        </Button>
      );
    case "detect_result": {
      const canPlan = phase.availability.status === "available";
      return (
        <>
          <Button variant="ghost" onClick={onClose} data-testid="terraform-cancel">
            {t("terraform.provision.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onGeneratePlan}
            disabled={!canPlan}
            data-testid="terraform-plan-cta"
          >
            {t("terraform.provision.plan.cta")}
          </Button>
        </>
      );
    }
    case "planning":
      return (
        <>
          <Button variant="ghost" onClick={onClose} disabled data-testid="terraform-cancel">
            {t("terraform.provision.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled
            data-testid="terraform-plan-cta"
          >
            {t("terraform.provision.plan.busy")}
          </Button>
        </>
      );
    case "plan_ready":
      return (
        <>
          <Button
            variant="ghost"
            onClick={onBackToOptions}
            data-testid="terraform-plan-back"
          >
            {t("terraform.provision.back_to_plan")}
          </Button>
          <Button variant="ghost" onClick={onClose} data-testid="terraform-cancel">
            {t("terraform.provision.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onApply}
            data-testid="terraform-apply-cta"
          >
            {phase.plan.no_changes
              ? t("terraform.provision.apply.cta")
              : `${t("terraform.provision.apply.cta")} (${policyVariant === "read_only_access" ? t("terraform.provision.policy.read_only_access") : t("terraform.provision.policy.security_audit")})`}
          </Button>
        </>
      );
    case "applying":
      return (
        <Button
          variant="primary"
          disabled
          data-testid="terraform-apply-cta"
        >
          {t("terraform.provision.apply.busy")}
        </Button>
      );
    case "applied":
      return (
        <Button
          variant="primary"
          onClick={onClose}
          data-testid="terraform-close"
        >
          {t("terraform.provision.close")}
        </Button>
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
  let availability: TerraformAvailability | null = null;
  if (phase.kind === "detect_result" || phase.kind === "planning" || phase.kind === "plan_ready" || phase.kind === "applying") {
    availability = phase.availability;
  }

  if (phase.kind === "detecting" || availability === null) {
    return (
      <p className="text-small text-saw-grey-600" data-testid="terraform-detect-state">
        {t("terraform.provision.detect.checking")}
      </p>
    );
  }

  if (availability.status === "available") {
    const shortSha = availability.sha256.slice(0, 12);
    return (
      <p
        className="flex items-center gap-2 text-small text-saw-grey-700"
        data-testid="terraform-detect-state"
      >
        <Badge tone="success">{t("terraform.provision.detect.available").replace("{sha}", shortSha)}</Badge>
      </p>
    );
  }

  if (availability.status === "missing") {
    return (
      <div
        role="alert"
        className="rounded-card border border-saw-grey-200 bg-saw-grey-50 px-3 py-2"
        data-testid="terraform-detect-state"
      >
        <p className="text-small font-medium text-saw-grey-800">
          {t("terraform.provision.detect.missing.title")}
        </p>
        <p className="mt-1 text-small text-saw-grey-700">
          {t("terraform.provision.detect.missing.body")}
        </p>
      </div>
    );
  }

  // integrity_failed
  return (
    <div
      role="alert"
      className="rounded-card border border-saw-grey-200 bg-saw-grey-100 px-3 py-2"
      data-testid="terraform-detect-state"
    >
      <p className="text-small font-medium text-saw-red">
        {t("terraform.provision.detect.integrity.title")}
      </p>
      <p className="mt-1 text-small text-saw-grey-800">
        {t("terraform.provision.detect.integrity.body")}
      </p>
    </div>
  );
}

function PolicySection({
  value,
  onChange,
  disabled,
}: {
  value: PolicyVariant;
  onChange: (v: PolicyVariant) => void;
  disabled: boolean;
}) {
  const t = useT();
  const options = useMemo(
    () =>
      [
        {
          value: "security_audit" as PolicyVariant,
          label: t("terraform.provision.policy.security_audit"),
        },
        {
          value: "read_only_access" as PolicyVariant,
          label: t("terraform.provision.policy.read_only_access"),
        },
      ],
    [t],
  );

  return (
    <div className="flex flex-col gap-2">
      <h3 className="text-small font-semibold text-saw-grey-800">
        {t("terraform.provision.policy.title")}
      </h3>
      <Select<PolicyVariant>
        label=""
        value={value}
        options={options}
        onChange={(v) => !disabled && onChange(v)}
        data-testid="terraform-policy-select"
        description={
          value === "security_audit"
            ? t("terraform.provision.policy.security_audit_hint")
            : t("terraform.provision.policy.read_only_access_hint")
        }
      />
      {value === "read_only_access" ? (
        <p
          role="alert"
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-900"
          data-testid="terraform-policy-warning"
        >
          <strong className="font-semibold">⚠</strong>{" "}
          {t("terraform.provision.policy.warning")}
        </p>
      ) : null}
    </div>
  );
}

function PlanSection({ plan }: { plan: PlanResult }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-3" data-testid="terraform-plan-section">
      <h3 className="text-small font-semibold text-saw-grey-800">
        {t("terraform.provision.plan.title")}
      </h3>

      <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-small">
        <dt className="text-saw-grey-500">
          {t("terraform.provision.plan.principal_label")}
        </dt>
        <dd className="font-mono break-all" data-testid="terraform-plan-principal">
          {plan.planned_principal_arn}
        </dd>
        <dt className="text-saw-grey-500">
          {t("terraform.provision.plan.policy_variant_label")}
        </dt>
        <dd>
          {plan.policy_variant === "security_audit"
            ? t("terraform.provision.policy.security_audit")
            : t("terraform.provision.policy.read_only_access")}
        </dd>
        <dt className="text-saw-grey-500">
          {t("terraform.provision.plan.created_at_label")}
        </dt>
        <dd>{formatTs(plan.created_at)}</dd>
        <dt className="text-saw-grey-500">
          {t("terraform.provision.plan.minted_token")}
        </dt>
        <dd className="font-mono text-saw-grey-700">
          {plan.plan_token.slice(0, 8)}…
        </dd>
      </dl>

      <p className="text-small text-saw-grey-700">
        {t("terraform.provision.plan.principal_hint")}
      </p>

      {plan.no_changes ? (
        <p
          className="rounded-card bg-saw-grey-50 px-3 py-2 text-small text-saw-grey-800"
          data-testid="terraform-plan-noop"
        >
          {t("terraform.provision.plan.no_changes")}
        </p>
      ) : (
        <ul
          className="divide-y divide-saw-grey-200 rounded-card border border-saw-grey-200 bg-saw-white"
          data-testid="terraform-plan-changes"
        >
          {plan.changes.map((c, idx) => (
            <PlanChangeRow key={`${c.resource_address}-${idx}`} change={c} />
          ))}
        </ul>
      )}
    </div>
  );
}

function PlanChangeRow({ change }: { change: PlanChange }) {
  const t = useT();
  const toneFor = (kind: PlanChange["kind"]) => {
    switch (kind) {
      case "create":
        return "success";
      case "update":
        return "info";
      case "delete":
      case "replace":
        return "danger";
      case "read":
        return "neutral";
      case "no_op":
        return "neutral";
    }
  };
  const verb = t(`terraform.provision.change.${change.kind}`);

  return (
    <li className="px-3 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={toneFor(change.kind)}>{verb}</Badge>
        <span className="font-mono text-small text-saw-grey-800">
          {change.resource_address}
        </span>
      </div>
      {change.attributes.length > 0 ? (
        <div className="mt-1 text-small text-saw-grey-600">
          <span className="text-saw-grey-500">
            {t("terraform.provision.attributes_label")}:
          </span>{" "}
          <span className="font-mono">{change.attributes.join(", ")}</span>
        </div>
      ) : null}
    </li>
  );
}

function AppliedSection({ result }: { result: ApplyResult }) {
  const t = useT();
  return (
    <div
      className="flex flex-col gap-2 rounded-card bg-saw-grey-50 px-4 py-3"
      data-testid="terraform-applied"
    >
      <p className="text-body font-semibold text-saw-grey-900">
        {t("terraform.provision.apply.success.title")}
      </p>
      <p className="text-small text-saw-grey-700">
        {t("terraform.provision.apply.success.body").replace(
          "{role_name}",
          result.role_name,
        )}
      </p>
      <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-small">
        <dt className="text-saw-grey-500">
          {t("terraform.provision.apply.success.role_arn_label")}
        </dt>
        <dd className="font-mono break-all" data-testid="terraform-applied-arn">
          {result.role_arn}
        </dd>
        <dt className="text-saw-grey-500">
          {t("terraform.provision.apply.success.trust_sha_label")}
        </dt>
        <dd className="font-mono text-saw-grey-700">
          {result.trust_policy_sha256.slice(0, 16)}…
        </dd>
      </dl>
    </div>
  );
}

function formatTs(ts: string): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString();
}
