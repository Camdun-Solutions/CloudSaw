// PR #69 — Generate-a-custom-report modal.
//
// Replaces the standalone `/custom_report` route. The user picks a
// date range, builds an account-scope list ({account, services[]}
// per click of "Add"), the output format (HTML or PDF), and the
// disclosure mode. Submitting opens the native save dialog and
// streams to the existing Rust IPC pipeline.
//
// Service-scope NOTE: the modal captures per-account service
// selections in local state so the user can express "all", "ec2 +
// s3 only", etc., but the IPC payload currently only forwards the
// account_ids — backend per-account service filtering is queued for
// a follow-up. The UI intent is preserved so we can wire it
// retroactively without re-prompting the user.

import { useEffect, useMemo, useState } from "react";

import { save } from "@tauri-apps/plugin-dialog";

import { Button, Modal, Select } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type Account,
  type AccountIdDisclosure,
  type ExportOutcome,
} from "@/lib/ipc";

/** PR #71: complete list of AWS services ScoutSuite can scope a scan
 *  to. Matches the directory layout under
 *  `vendor/scoutsuite/ScoutSuite/providers/aws/services/`, plus the
 *  prefix taxonomy the scanner emits (`iam-`, `s3-`, etc.). The label
 *  map handles display capitalization — special-cased acronyms and
 *  CamelCase brand names where AWS uses them. */
const KNOWN_SERVICES: ReadonlyArray<string> = [
  "acm",
  "awslambda",
  "cloudformation",
  "cloudfront",
  "cloudtrail",
  "cloudwatch",
  "codebuild",
  "config",
  "directconnect",
  "dynamodb",
  "ec2",
  "ecr",
  "ecs",
  "efs",
  "eks",
  "elasticache",
  "elasticbeanstalk",
  "elb",
  "elbv2",
  "emr",
  "guardduty",
  "iam",
  "kms",
  "opensearch",
  "rds",
  "redshift",
  "route53",
  "s3",
  "secretsmanager",
  "ses",
  "sns",
  "sqs",
  "ssm",
  "stepfunctions",
  "sts",
  "vpc",
];

/** PR #71: per-service display label. Acronyms stay all-caps where
 *  AWS uses them (IAM, RDS, KMS, VPC, EC2, ELB, ELBv2, …); brand
 *  CamelCase names follow AWS marketing (CloudWatch, CloudFront,
 *  DynamoDB, …). Anything not in this table falls through to a
 *  capitalize-first-letter helper. */
const SERVICE_LABELS: Record<string, string> = {
  acm: "ACM",
  awslambda: "AWS Lambda",
  cloudformation: "CloudFormation",
  cloudfront: "CloudFront",
  cloudtrail: "CloudTrail",
  cloudwatch: "CloudWatch",
  codebuild: "CodeBuild",
  directconnect: "Direct Connect",
  dynamodb: "DynamoDB",
  ec2: "EC2",
  ecr: "ECR",
  ecs: "ECS",
  efs: "EFS",
  eks: "EKS",
  elasticache: "ElastiCache",
  elasticbeanstalk: "Elastic Beanstalk",
  elb: "ELB",
  elbv2: "ELBv2",
  emr: "EMR",
  guardduty: "GuardDuty",
  iam: "IAM",
  kms: "KMS",
  opensearch: "OpenSearch",
  rds: "RDS",
  redshift: "Redshift",
  route53: "Route53",
  s3: "S3",
  secretsmanager: "Secrets Manager",
  ses: "SES",
  sns: "SNS",
  sqs: "SQS",
  ssm: "SSM",
  stepfunctions: "Step Functions",
  sts: "STS",
  vpc: "VPC",
};

function labelForService(svc: string): string {
  return (
    SERVICE_LABELS[svc] ?? svc.charAt(0).toUpperCase() + svc.slice(1)
  );
}

type Format = "html" | "pdf";

/** One row in the Account Scope list. `services` empty = "All". */
type ScopeEntry = { account_id: string; services: string[] };

type Props = {
  open: boolean;
  onClose: () => void;
  /** Called when the export pipeline returns a path. Lets the host
   *  (Reports section) refresh any "recent exports" UI it owns. */
  onExported?: (outcome: ExportOutcome) => void;
};

export default function CustomReportModal({ open, onClose, onExported }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const today = new Date().toISOString().slice(0, 10);
  const ninetyDaysAgo = new Date(Date.now() - 90 * 24 * 60 * 60 * 1000)
    .toISOString()
    .slice(0, 10);

  const [start, setStart] = useState(ninetyDaysAgo);
  const [end, setEnd] = useState(today);
  const [format, setFormat] = useState<Format>("html");
  const [showFullIds, setShowFullIds] = useState(false);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [scope, setScope] = useState<ScopeEntry[]>([]);
  const [pendingAccount, setPendingAccount] = useState<string>("");
  const [pendingServices, setPendingServices] = useState<string[]>([]);
  const [pendingAllServices, setPendingAllServices] = useState(true);
  const [adding, setAdding] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ExportOutcome | null>(null);

  // Load accounts when the modal opens.
  useEffect(() => {
    if (!open) return;
    setOutcome(null);
    setError(null);
    let cancelled = false;
    ipc
      .accountsList()
      .then((list) => {
        if (!cancelled) setAccounts(list);
      })
      .catch((err) => {
        if (!cancelled) setError(formatError(err));
      });
    return () => {
      cancelled = true;
    };
  }, [open, formatError]);

  // Accounts that haven't been added to scope yet — the "Add" picker
  // hides duplicates so the user can't add the same account twice.
  const unscopedAccounts = useMemo(
    () =>
      accounts.filter(
        (a) => !scope.some((s) => s.account_id === a.aws_account_id),
      ),
    [accounts, scope],
  );

  function startAdd() {
    setAdding(true);
    setPendingAccount(unscopedAccounts[0]?.aws_account_id ?? "");
    setPendingServices([]);
    setPendingAllServices(true);
  }

  function commitAdd() {
    if (!pendingAccount) {
      setAdding(false);
      return;
    }
    setScope([
      ...scope,
      {
        account_id: pendingAccount,
        services: pendingAllServices ? [] : pendingServices,
      },
    ]);
    setAdding(false);
    setPendingAccount("");
    setPendingServices([]);
    setPendingAllServices(true);
  }

  function removeScope(idx: number) {
    setScope(scope.filter((_, i) => i !== idx));
  }

  function toggleService(svc: string) {
    setPendingServices((cur) =>
      cur.includes(svc) ? cur.filter((s) => s !== svc) : [...cur, svc],
    );
  }

  async function buildAndExport() {
    setError(null);
    setOutcome(null);
    // When the scope list is empty we send `[]` to the IPC — the
    // existing aggregator treats that as "all locally-known accounts".
    const accountScope: string[] = scope.map((s) => s.account_id);
    const disclosure: AccountIdDisclosure = showFullIds ? "full" : "masked";

    let picked: string | null = null;
    try {
      const ext = format === "pdf" ? "pdf" : "html";
      const defaultPath = `cloudsaw-custom-${start}-to-${end}.${ext}`;
      const result = await save({
        defaultPath,
        filters: [
          format === "pdf"
            ? { name: "PDF", extensions: ["pdf"] }
            : { name: "HTML", extensions: ["html", "htm"] },
        ],
      });
      if (result && typeof result === "string") picked = result;
    } catch (e) {
      setError(formatError(e));
      return;
    }
    if (!picked) return; // user canceled the save dialog

    setSubmitting(true);
    try {
      const startIso = `${start}T00:00:00Z`;
      const endIso = `${end}T23:59:59Z`;
      const fn =
        format === "pdf"
          ? ipc.reportExportCustomPdf
          : ipc.reportExportCustomHtml;
      const r = await fn(startIso, endIso, accountScope, picked, disclosure);
      setOutcome(r);
      onExported?.(r);
      // PR #71: close the modal once the report finishes generating.
      // Give the success state ~700ms to read before the dialog
      // dismisses so the user has a beat of visual confirmation.
      window.setTimeout(() => {
        onClose();
      }, 700);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setSubmitting(false);
    }
  }

  function close() {
    if (submitting) return;
    onClose();
  }

  const accountById = (id: string) =>
    accounts.find((a) => a.aws_account_id === id);

  return (
    <Modal
      open={open}
      onClose={close}
      title={t("report.custom.modal_title")}
      size="lg"
      footer={
        <>
          <Button
            variant="ghost"
            onClick={close}
            disabled={submitting}
            data-testid="custom-report-cancel"
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void buildAndExport()}
            disabled={submitting || !start || !end}
            data-testid="custom-report-go"
          >
            {submitting
              ? t("report.export.submitting")
              : t("report.custom.go")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
            <span>{t("report.custom.start")}</span>
            <input
              type="date"
              value={start}
              onChange={(e) => setStart(e.target.value)}
              className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
              data-testid="custom-report-start"
            />
          </label>
          <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
            <span>{t("report.custom.end")}</span>
            <input
              type="date"
              value={end}
              onChange={(e) => setEnd(e.target.value)}
              className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
              data-testid="custom-report-end"
            />
          </label>
        </div>

        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <span className="text-small font-medium text-saw-grey-900 dark:text-saw-beige">
              {t("report.custom.account_scope")}
            </span>
            <Button
              variant="secondary"
              size="sm"
              onClick={startAdd}
              disabled={adding || unscopedAccounts.length === 0}
              data-testid="custom-report-scope-add"
            >
              {t("report.custom.scope_add_cta")}
            </Button>
          </div>
          <p className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("report.custom.empty_scope_hint")}
          </p>

          {scope.length === 0 && !adding ? (
            <p
              className="rounded-card border border-dashed border-saw-grey-200 dark:border-saw-grey-700 px-3 py-2 text-small text-saw-grey-600 dark:text-saw-grey-400"
              data-testid="custom-report-scope-empty"
            >
              {t("report.custom.scope_empty")}
            </p>
          ) : (
            <ul className="flex flex-col gap-2" data-testid="custom-report-scope-list">
              {scope.map((entry, idx) => {
                const account = accountById(entry.account_id);
                return (
                  <li
                    key={entry.account_id}
                    className="flex items-start justify-between gap-3 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-grey-800 px-3 py-2"
                    data-testid={`custom-report-scope-row-${entry.account_id}`}
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-small font-medium text-saw-grey-900 dark:text-saw-beige">
                        {account?.label ?? entry.account_id}
                      </p>
                      <p className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
                        {entry.services.length === 0
                          ? t("report.custom.services_all")
                          : entry.services.map(labelForService).join(", ")}
                      </p>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => removeScope(idx)}
                      data-testid={`custom-report-scope-remove-${entry.account_id}`}
                    >
                      {t("common.remove")}
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}

          {adding ? (
            <div
              className="flex flex-col gap-3 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-grey-800 px-3 py-3"
              data-testid="custom-report-scope-pending"
            >
              <Select<string>
                label={t("report.custom.pick_account")}
                value={pendingAccount}
                options={unscopedAccounts.map((a) => ({
                  value: a.aws_account_id,
                  label: `${a.label} (${a.aws_account_id})`,
                }))}
                onChange={(v) => setPendingAccount(v)}
                data-testid="custom-report-pick-account"
              />
              <div>
                <label className="flex items-center gap-2 text-small text-saw-grey-700 dark:text-saw-grey-300">
                  <input
                    type="checkbox"
                    checked={pendingAllServices}
                    onChange={(e) => setPendingAllServices(e.target.checked)}
                    data-testid="custom-report-services-all"
                  />
                  <span>{t("report.custom.services_all")}</span>
                </label>
                {!pendingAllServices ? (
                  <div
                    // PR #71: dark-theme adaptive checkbox styling +
                    // proper service labels (CloudWatch, IAM, ELBv2,
                    // …) replace the previous lowercase mono prefix.
                    className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1.5 sm:grid-cols-3"
                    data-testid="custom-report-services-list"
                  >
                    {KNOWN_SERVICES.map((svc) => (
                      <label
                        key={svc}
                        className="flex items-center gap-2 rounded px-1.5 py-0.5 text-small text-saw-grey-700 hover:bg-saw-grey-100 dark:text-saw-grey-300 dark:hover:bg-saw-grey-800"
                      >
                        <input
                          type="checkbox"
                          checked={pendingServices.includes(svc)}
                          onChange={() => toggleService(svc)}
                          data-testid={`custom-report-service-${svc}`}
                          className="h-4 w-4 rounded border-saw-grey-300 bg-saw-white text-saw-red focus:ring-saw-red dark:border-saw-grey-600 dark:bg-saw-grey-800 dark:checked:bg-saw-red dark:focus:ring-saw-red"
                        />
                        <span>{labelForService(svc)}</span>
                      </label>
                    ))}
                  </div>
                ) : null}
              </div>
              <div className="flex justify-end gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setAdding(false)}
                  data-testid="custom-report-pending-cancel"
                >
                  {t("common.cancel")}
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={commitAdd}
                  disabled={!pendingAccount}
                  data-testid="custom-report-pending-commit"
                >
                  {t("report.custom.scope_add_commit")}
                </Button>
              </div>
            </div>
          ) : null}
        </div>

        <fieldset>
          <legend className="text-saw-grey-900 dark:text-saw-beige font-medium text-small">
            {t("report.export.format_label")}
          </legend>
          <label className="mr-4 text-small text-saw-grey-700 dark:text-saw-grey-300">
            <input
              type="radio"
              name="custom-report-format"
              checked={format === "html"}
              onChange={() => setFormat("html")}
              data-testid="custom-report-format-html"
            />{" "}
            {t("report.export.format.html")}
          </label>
          <label className="text-small text-saw-grey-700 dark:text-saw-grey-300">
            <input
              type="radio"
              name="custom-report-format"
              checked={format === "pdf"}
              onChange={() => setFormat("pdf")}
              data-testid="custom-report-format-pdf"
            />{" "}
            {t("report.export.format.pdf")}
          </label>
        </fieldset>

        <label className="flex items-start gap-2 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <input
            type="checkbox"
            checked={showFullIds}
            onChange={(e) => setShowFullIds(e.target.checked)}
            className="mt-1"
            data-testid="custom-report-disclosure"
          />
          <span>
            <span className="font-medium text-saw-grey-900 dark:text-saw-beige">
              {t("report.export.disclosure_label")}
            </span>
            <br />
            <span className="text-xs text-saw-grey-600 dark:text-saw-grey-400">
              {t("report.export.disclosure_hint")}
            </span>
          </span>
        </label>

        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            data-testid="custom-report-error"
          >
            {error}
          </p>
        ) : null}
        {outcome ? (
          <div
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="custom-report-outcome"
          >
            <p>
              {t("report.export.success_body")
                .replace("{bytes}", outcome.bytes_written.toLocaleString())
                .replace("{path}", outcome.primary_path)}
            </p>
          </div>
        ) : null}
      </div>
    </Modal>
  );
}
