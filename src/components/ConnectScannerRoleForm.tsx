// "Connect scanner role" form — Phase 2 replacement for the deleted
// ProvisionScannerRoleModal.
//
// CloudSaw no longer creates the IAM role itself. The user creates it
// in their environment (Console, Terraform, CloudFormation, or AWS
// CLI — four recipes rendered with values pre-substituted), then
// pastes the resulting ARN back here. The Validate & Connect button
// runs a dry-run `sts:AssumeRole` and persists the role on success.
//
// The component is shared between the onboarding wizard's step 4 and
// the per-account modal triggered from the Accounts page. The two
// surfaces differ only in their chrome (inline vs. modal) — the form
// itself is identical.

import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type ConnectResult,
  type PolicyVariant,
  type RoleRequirements,
} from "@/lib/ipc";

type Props = {
  awsAccountId: string;
  /** Optional callback fired after a successful connect so the parent
   * (onboarding wizard or Accounts page) can refresh its state. */
  onConnected?: (result: ConnectResult) => void;
};

type FormState =
  | { kind: "loading_requirements" }
  | { kind: "requirements_failed"; message: string }
  | { kind: "ready"; requirements: RoleRequirements }
  | { kind: "submitting"; requirements: RoleRequirements }
  | { kind: "succeeded"; result: ConnectResult }
  | { kind: "failed"; requirements: RoleRequirements; message: string };

export default function ConnectScannerRoleForm({ awsAccountId, onConnected }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [state, setState] = useState<FormState>({ kind: "loading_requirements" });
  const [roleArn, setRoleArn] = useState("");
  const [policyVariant, setPolicyVariant] =
    useState<PolicyVariant>("security_audit");

  const loadRequirements = useCallback(async () => {
    setState({ kind: "loading_requirements" });
    try {
      const reqs = await ipc.scannerRoleRequirements(awsAccountId);
      setPolicyVariant(reqs.default_policy_variant);
      setState({ kind: "ready", requirements: reqs });
    } catch (e) {
      setState({ kind: "requirements_failed", message: formatError(e) });
    }
  }, [awsAccountId, formatError]);

  useEffect(() => {
    void loadRequirements();
  }, [loadRequirements]);

  async function submit() {
    if (state.kind !== "ready" && state.kind !== "failed") return;
    const reqs = state.requirements;
    setState({ kind: "submitting", requirements: reqs });
    try {
      const result = await ipc.scannerRoleConnect(
        awsAccountId,
        roleArn.trim(),
        policyVariant,
      );
      setState({ kind: "succeeded", result });
      onConnected?.(result);
    } catch (e) {
      setState({
        kind: "failed",
        requirements: reqs,
        message: formatError(e),
      });
    }
  }

  // --- Render branches ---------------------------------------------------

  if (state.kind === "loading_requirements") {
    return (
      <p
        className="text-small text-saw-grey-700 dark:text-saw-grey-300"
        data-testid="scanner-role-form-loading"
      >
        {t("common.loading")}
      </p>
    );
  }

  if (state.kind === "requirements_failed") {
    return (
      <div
        role="alert"
        className="rounded-card border border-saw-red/40 bg-saw-red/5 px-3 py-2 text-small text-saw-grey-900 dark:text-saw-beige"
        data-testid="scanner-role-form-requirements-failed"
      >
        {state.message}
      </div>
    );
  }

  if (state.kind === "succeeded") {
    return (
      <div
        className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-3 py-3 text-small text-saw-grey-800 dark:text-saw-beige"
        data-testid="scanner-role-form-success"
      >
        <p className="font-semibold text-saw-grey-900 dark:text-saw-beige">
          {t("scanner_role.success.title")}
        </p>
        <p className="mt-1 text-saw-grey-700 dark:text-saw-grey-300">
          {t("scanner_role.success.body")
            .replace("{accountId}", awsAccountId)
            .replace("{roleArn}", state.result.role_arn)}
        </p>
      </div>
    );
  }

  const requirements =
    state.kind === "ready" ||
    state.kind === "submitting" ||
    state.kind === "failed"
      ? state.requirements
      : null;
  const submitting = state.kind === "submitting";
  const errorMessage = state.kind === "failed" ? state.message : null;
  // `requirements` is non-null on every branch we reach below.
  if (!requirements) return null;

  return (
    <div className="flex flex-col gap-4">
      <RequirementsCard
        trustedPrincipalArn={requirements.trusted_principal_arn}
        externalId={requirements.external_id}
        policyVariant={policyVariant}
      />
      <RecipesAccordion
        trustedPrincipalArn={requirements.trusted_principal_arn}
        externalId={requirements.external_id}
        policyVariant={policyVariant}
      />
      <ConnectForm
        roleArn={roleArn}
        setRoleArn={setRoleArn}
        policyVariant={policyVariant}
        setPolicyVariant={setPolicyVariant}
        onSubmit={() => void submit()}
        submitting={submitting}
        errorMessage={errorMessage}
      />
    </div>
  );
}

// --- Sub-components ----------------------------------------------------

function RequirementsCard({
  trustedPrincipalArn,
  externalId,
  policyVariant,
}: {
  trustedPrincipalArn: string;
  externalId: string;
  policyVariant: PolicyVariant;
}) {
  const t = useT();
  return (
    <section
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black p-4 text-small text-saw-grey-800 dark:text-saw-beige"
      aria-labelledby="scanner-role-requirements-title"
      data-testid="scanner-role-requirements"
    >
      <h3
        id="scanner-role-requirements-title"
        className="text-body font-semibold text-saw-grey-900 dark:text-saw-beige"
      >
        {t("scanner_role.requirements.title")}
      </h3>
      <p className="mt-1 text-saw-grey-700">
        {t("scanner_role.requirements.body")}
      </p>
      <dl className="mt-3 flex flex-col gap-2">
        <RequirementRow
          label={t("scanner_role.requirements.trusted_principal_label")}
          value={trustedPrincipalArn}
          testId="scanner-role-req-trusted-principal"
        />
        <RequirementRow
          label={t("scanner_role.requirements.external_id_label")}
          value={externalId}
          testId="scanner-role-req-external-id"
        />
        <RequirementRow
          label={t("scanner_role.requirements.policy_label")}
          value={managedPolicyArnFor(policyVariant)}
          testId="scanner-role-req-policy"
        />
      </dl>
    </section>
  );
}

function RequirementRow({
  label,
  value,
  testId,
}: {
  label: string;
  value: string;
  testId: string;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);

  function copy() {
    if (!navigator.clipboard) return;
    void navigator.clipboard.writeText(value).then(
      () => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 2000);
      },
      () => undefined,
    );
  }

  return (
    <div className="flex flex-col gap-1">
      <dt className="text-saw-grey-500 dark:text-saw-grey-400">{label}</dt>
      <dd className="flex items-center gap-2">
        <code
          className="flex-1 rounded bg-saw-white dark:bg-saw-grey-dark px-2 py-1 font-mono text-xs text-saw-grey-900 dark:text-saw-beige break-all"
          data-testid={testId}
        >
          {value}
        </code>
        <button
          type="button"
          onClick={copy}
          className="rounded border border-saw-grey-200 dark:border-saw-grey-700 px-2 py-1 text-xs text-saw-grey-700 dark:text-saw-grey-300 hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800"
          data-testid={`${testId}-copy`}
        >
          {copied
            ? t("scanner_role.requirements.copied")
            : t("scanner_role.requirements.copy")}
        </button>
      </dd>
    </div>
  );
}

function RecipesAccordion({
  trustedPrincipalArn,
  externalId,
  policyVariant,
}: {
  trustedPrincipalArn: string;
  externalId: string;
  policyVariant: PolicyVariant;
}) {
  const t = useT();
  const policyArn = managedPolicyArnFor(policyVariant);

  const trustPolicyJson = JSON.stringify(
    {
      Version: "2012-10-17",
      Statement: [
        {
          Effect: "Allow",
          Principal: { AWS: trustedPrincipalArn },
          Action: "sts:AssumeRole",
          Condition: { StringEquals: { "sts:ExternalId": externalId } },
        },
      ],
    },
    null,
    2,
  );

  return (
    <section
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-4"
      aria-labelledby="scanner-role-recipes-title"
      data-testid="scanner-role-recipes"
    >
      <h3
        id="scanner-role-recipes-title"
        className="text-body font-semibold text-saw-grey-900 dark:text-saw-beige"
      >
        {t("scanner_role.recipes.title")}
      </h3>
      <p className="mt-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
        {t("scanner_role.recipes.body")}
      </p>
      <div className="mt-3 flex flex-col gap-2">
        <RecipeDetails
          title={t("scanner_role.recipes.console.title")}
          testId="scanner-role-recipe-console"
        >
          <ol className="ml-4 list-decimal text-small text-saw-grey-800 dark:text-saw-beige">
            <li>{t("scanner_role.recipes.console.step1")}</li>
            <li>{t("scanner_role.recipes.console.step2")}</li>
            <li>{t("scanner_role.recipes.console.step3")}</li>
            <li>{t("scanner_role.recipes.console.step4")}</li>
            <li>{t("scanner_role.recipes.console.step5")}</li>
          </ol>
          <p className="mt-3 text-small text-saw-grey-700 dark:text-saw-grey-300">
            {t("scanner_role.recipes.console.trust_policy_label")}
          </p>
          <CodeBlock value={trustPolicyJson} />
        </RecipeDetails>

        <RecipeDetails
          title={t("scanner_role.recipes.terraform.title")}
          testId="scanner-role-recipe-terraform"
        >
          <p className="text-small text-saw-grey-700 dark:text-saw-grey-300">
            {t("scanner_role.recipes.terraform.body")}
          </p>
          <CodeBlock
            value={`module "cloudsaw_scanner_role" {
  source = "github.com/Camdun-Solutions/CloudSaw//src-tauri/tf-modules/scanner-role?ref=master"

  trusted_principal_arn = "${trustedPrincipalArn}"
  external_id           = "${externalId}"
  policy_variant        = "${policyVariant}"
}

output "scanner_role_arn" {
  value = module.cloudsaw_scanner_role.role_arn
}`}
          />
        </RecipeDetails>

        <RecipeDetails
          title={t("scanner_role.recipes.cloudformation.title")}
          testId="scanner-role-recipe-cloudformation"
        >
          <p className="text-small text-saw-grey-700 dark:text-saw-grey-300">
            {t("scanner_role.recipes.cloudformation.body")}
          </p>
          <CodeBlock
            value={`AWSTemplateFormatVersion: "2010-09-09"
Description: CloudSaw scanner role

Resources:
  CloudSawScannerRole:
    Type: AWS::IAM::Role
    Properties:
      RoleName: CloudSawScannerRole
      AssumeRolePolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Effect: Allow
            Principal:
              AWS: ${trustedPrincipalArn}
            Action: sts:AssumeRole
            Condition:
              StringEquals:
                sts:ExternalId: "${externalId}"
      ManagedPolicyArns:
        - ${policyArn}

Outputs:
  ScannerRoleArn:
    Value: !GetAtt CloudSawScannerRole.Arn`}
          />
        </RecipeDetails>

        <RecipeDetails
          title={t("scanner_role.recipes.cli.title")}
          testId="scanner-role-recipe-cli"
        >
          <p className="text-small text-saw-grey-700 dark:text-saw-grey-300">
            {t("scanner_role.recipes.cli.body")}
          </p>
          <CodeBlock
            value={`# Save the trust policy JSON to a file:
cat > trust-policy.json <<'EOF'
${trustPolicyJson}
EOF

# Create the role and attach the managed policy:
aws iam create-role \\
  --role-name CloudSawScannerRole \\
  --assume-role-policy-document file://trust-policy.json

aws iam attach-role-policy \\
  --role-name CloudSawScannerRole \\
  --policy-arn ${policyArn}

# Print the role ARN you'll paste back into CloudSaw:
aws iam get-role --role-name CloudSawScannerRole \\
  --query 'Role.Arn' --output text`}
          />
        </RecipeDetails>
      </div>
    </section>
  );
}

function RecipeDetails({
  title,
  testId,
  children,
}: {
  title: string;
  testId: string;
  children: React.ReactNode;
}) {
  return (
    <details
      className="rounded border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black"
      data-testid={testId}
    >
      <summary className="cursor-pointer px-3 py-2 text-small font-semibold text-saw-grey-900 dark:text-saw-beige">
        {title}
      </summary>
      <div className="px-3 py-3">{children}</div>
    </details>
  );
}

function CodeBlock({ value }: { value: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  function copy() {
    if (!navigator.clipboard) return;
    void navigator.clipboard.writeText(value).then(
      () => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 2000);
      },
      () => undefined,
    );
  }
  return (
    <div className="relative mt-2 rounded bg-saw-grey-900 p-3">
      <button
        type="button"
        onClick={copy}
        className="absolute right-2 top-2 rounded bg-saw-grey-700 px-2 py-1 text-xs text-saw-grey-100 hover:bg-saw-grey-600"
        data-testid="scanner-role-codeblock-copy"
      >
        {copied
          ? t("scanner_role.requirements.copied")
          : t("scanner_role.requirements.copy")}
      </button>
      <pre className="overflow-x-auto font-mono text-xs text-saw-grey-50">
        {value}
      </pre>
    </div>
  );
}

function ConnectForm({
  roleArn,
  setRoleArn,
  policyVariant,
  setPolicyVariant,
  onSubmit,
  submitting,
  errorMessage,
}: {
  roleArn: string;
  setRoleArn: (v: string) => void;
  policyVariant: PolicyVariant;
  setPolicyVariant: (v: PolicyVariant) => void;
  onSubmit: () => void;
  submitting: boolean;
  errorMessage: string | null;
}) {
  const t = useT();
  const canSubmit = !submitting && /^arn:aws:iam::\d{12}:role\/.+/.test(roleArn.trim());

  return (
    <section
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-4"
      aria-labelledby="scanner-role-form-title"
      data-testid="scanner-role-form"
    >
      <h3
        id="scanner-role-form-title"
        className="text-body font-semibold text-saw-grey-900 dark:text-saw-beige"
      >
        {t("scanner_role.form.title")}
      </h3>
      <label className="mt-3 flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
        <span>{t("scanner_role.form.role_arn_label")}</span>
        <input
          type="text"
          value={roleArn}
          onChange={(e) => setRoleArn(e.target.value)}
          placeholder={t("scanner_role.form.role_arn_placeholder")}
          className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 font-mono text-body text-saw-grey-900 dark:text-saw-beige"
          data-testid="scanner-role-form-arn-input"
          disabled={submitting}
        />
      </label>
      <fieldset className="mt-3 flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
        <legend>{t("scanner_role.form.policy_variant_label")}</legend>
        <label className="flex items-center gap-2">
          <input
            type="radio"
            name="policy-variant"
            value="security_audit"
            checked={policyVariant === "security_audit"}
            onChange={() => setPolicyVariant("security_audit")}
            disabled={submitting}
            data-testid="scanner-role-form-policy-security-audit"
          />
          <span>{t("scanner_role.form.policy_security_audit")}</span>
        </label>
        <label className="flex items-center gap-2">
          <input
            type="radio"
            name="policy-variant"
            value="read_only_access"
            checked={policyVariant === "read_only_access"}
            onChange={() => setPolicyVariant("read_only_access")}
            disabled={submitting}
            data-testid="scanner-role-form-policy-read-only-access"
          />
          <span>{t("scanner_role.form.policy_read_only_access")}</span>
        </label>
      </fieldset>
      <div className="mt-4">
        <Button
          variant="primary"
          onClick={onSubmit}
          disabled={!canSubmit}
          data-testid="scanner-role-form-submit"
        >
          {submitting
            ? t("scanner_role.form.submitting")
            : t("scanner_role.form.submit")}
        </Button>
      </div>
      {errorMessage ? (
        <p
          role="alert"
          className="mt-3 rounded-card border border-saw-red/40 bg-saw-red/5 px-3 py-2 text-small text-saw-grey-900 dark:text-saw-beige"
          data-testid="scanner-role-form-error"
        >
          {errorMessage}
        </p>
      ) : null}
    </section>
  );
}

function managedPolicyArnFor(variant: PolicyVariant): string {
  return variant === "read_only_access"
    ? "arn:aws:iam::aws:policy/ReadOnlyAccess"
    : "arn:aws:iam::aws:policy/SecurityAudit";
}
