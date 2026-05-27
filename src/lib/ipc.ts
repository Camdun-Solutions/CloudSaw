// Single typed IPC client. Every component talks to the Rust backend
// through this module — no direct `invoke()` calls live in components, hooks,
// or routes (enforced by CONTRIBUTING.md and CI lint).
//
// Each method here corresponds 1:1 to a `#[tauri::command]` declared in
// src-tauri/src/ipc/mod.rs. Inputs and outputs are plain serializable shapes.

import { invoke } from "@tauri-apps/api/core";

/** Stable error shape returned by every backend command that can fail. */
export type IpcError = {
  code: string;
  message: string;
};

/** Re-lock cadence. The shape matches the Rust `LockPeriod` enum exactly so
 * `serde` round-trips it without an adapter. */
export type LockPeriod =
  | { kind: "immediate" }
  | { kind: "after"; seconds: number }
  | { kind: "never" };

export type BiometricAvailability = "Available" | "Unconfigured" | "Unavailable";

export type LockSettings = {
  lock_period: LockPeriod;
  biometric_enabled: boolean;
};

export type LockState = {
  first_run: boolean;
  locked: boolean;
  settings: LockSettings;
  biometric_availability: BiometricAvailability;
  recovery_available: boolean;
};

// --- AWS auth (Contract 03) ----------------------------------------------

/** Whether a profile is a vanilla AWS CLI profile or one backed by IAM
 * Identity Center (SSO). Drives a UI badge only — auth resolution uses
 * the SDK provider chain either way. */
export type ProfileSource = "cli" | "sso";

export type ProfileInfo = {
  name: string;
  source: ProfileSource;
};

/** Result of `sts:GetCallerIdentity`. Account/user IDs and ARN are
 * returned in full so the UI can confirm exactly which identity was
 * resolved — backend logs and error surfaces redact these values. */
export type CallerIdentity = {
  account_id: string;
  user_id: string;
  arn: string;
};

export type TestFailureReason =
  | "profile_not_configured"
  | "sso_expired"
  | "permission_denied"
  | "connectivity"
  | "timeout"
  | "other";

/** Discriminated union returned by `auth_test_profile`. Switch on `status`. */
export type ProfileTestResult =
  | { status: "success"; identity: CallerIdentity }
  | { status: "failure"; reason: TestFailureReason; api: string | null };

// --- Multi-account (Contract 04) -----------------------------------------

export type Environment = "dev" | "staging" | "prod" | "other";
export type ScanOutcome = "success" | "failure" | "partial_success" | "unknown";

/** One row of the local `accounts` table. The `aws_account_id` is the
 * verified 12-digit AWS account ID and serves as the partitioning key for
 * every account-scoped table added by later contracts. */
export type Account = {
  aws_account_id: string;
  label: string;
  profile_name: string;
  environment: Environment;
  role_provisioned: boolean;
  role_provisioned_at: string | null;
  last_scan_at: string | null;
  last_scan_status: ScanOutcome | null;
  created_at: string;
  updated_at: string;
};

export type AddAccountInput = {
  label: string;
  profile_name: string;
  environment: Environment;
};

export type UpdateAccountInput = {
  aws_account_id: string;
  label: string;
  profile_name: string;
  environment: Environment;
};

/** Data-impact preview returned from `accounts_remove`. `was_active` tells
 * the UI whether to prompt for a new active selection. */
export type RemovalImpact = {
  scans: number;
  findings: number;
  tf_work: number;
  was_active: boolean;
};

export type AccountsDisplaySettings = {
  reveal_full_ids: boolean;
};

/** Mask a 12-digit AWS account ID to the last 4 digits. Mirrors the Rust
 * `accounts::mask_for_logs` helper so the UI default and the log format
 * stay aligned. */
export function maskAccountId(id: string): string {
  if (id.length < 4) return "****";
  return `****${id.slice(-4)}`;
}

// --- Scanner-role connect flow (Phase 2 — replaces Terraform Contract 05) --

/** Which AWS-managed scanner policy the user attached. `security_audit` is
 * the least-privilege default surfaced in the recipes; `read_only_access`
 * is an explicit opt-in radio with a warning carried over from Contract 05. */
export type PolicyVariant = "security_audit" | "read_only_access";

/** Values the UI substitutes into the four setup recipes (Console / Terraform
 * / CloudFormation / AWS CLI). `external_id` is generated per-account and
 * MUST be used verbatim by the user in their role's trust policy condition. */
export type RoleRequirements = {
  trusted_principal_arn: string;
  external_id: string;
  default_policy_variant: PolicyVariant;
};

/** Returned by `scanner_role_connect`. Same shape as the deleted
 * `ApplyResult` so consumers don't need to change. `trust_policy_sha256` is
 * `null` when the connecting profile lacks `iam:GetRole` (graceful-degradation
 * path); the connect itself still succeeds because the AssumeRole dry-run is
 * the authoritative validation. */
export type ConnectResult = {
  role_arn: string;
  role_name: string;
  policy_variant: PolicyVariant;
  trust_policy_sha256: string | null;
};

/** Discriminated union returned by `scanner_role_status`. Shape carried over
 * from the deleted `terraform_provisioning_status` — drives the same
 * "Connect / Re-connect / Show last error" UI affordances. */
export type ProvisioningStatus =
  | { status: "not_provisioned" }
  | {
      status: "provisioned";
      role_arn: string;
      policy_variant: PolicyVariant;
      provisioned_at: string;
    }
  | { status: "failed"; last_error_code: string; attempted_at: string };

// --- Scanner orchestrator (Contract 06) ----------------------------------

/** Outcome of `scanner_detect`. The bundled ScoutSuite binary is gated behind
 * a build-pinned SHA-256, so a tampered or missing binary blocks the scan UI
 * before any AWS call happens. */
export type ScoutSuiteAvailability =
  | { status: "available"; sha256: string }
  | { status: "missing" }
  | { status: "integrity_failed" };

/** Lifecycle state of a scan. See Contract 06 §Expected Output for the
 * transition graph. Terminal states are `complete`, `complete_with_warnings`,
 * `failed`, and `canceled`. */
export type ScanStatus =
  | "pending"
  | "assuming_role"
  | "scanning"
  | "parsing"
  | "complete"
  | "complete_with_warnings"
  | "failed"
  | "canceled";

/** One scan record. `raw_output_path` is set once the scan reaches `parsing`
 * or any terminal state; until then it's null. The frontend never reads this
 * file directly — Contract 07's parser owns it. */
export type ScanRecord = {
  scan_id: string;
  aws_account_id: string;
  status: ScanStatus;
  started_at: string;
  finished_at: string | null;
  failure_code: string | null;
  warning_code: string | null;
  warning_detail: string | null;
  raw_output_path: string | null;
  role_session_name: string;
  truncated: boolean;
};

/** Terminal states for which the UI no longer polls `scan_status`. */
export const TERMINAL_SCAN_STATUSES: ReadonlySet<ScanStatus> = new Set([
  "complete",
  "complete_with_warnings",
  "failed",
  "canceled",
]);

export function isTerminalScanStatus(s: ScanStatus): boolean {
  return TERMINAL_SCAN_STATUSES.has(s);
}

// --- Findings parser & store (Contract 07) --------------------------------

/** Normalized five-tier severity. The frontend NEVER conveys severity by
 * color alone (WCAG 2.1 AA + Contract 09 §Constraints); every UI surface
 * also renders the severity word. */
export type Severity =
  | "critical"
  | "high"
  | "medium"
  | "low"
  | "informational";

export const SEVERITY_ORDER: readonly Severity[] = [
  "critical",
  "high",
  "medium",
  "low",
  "informational",
] as const;

/** Finding lifecycle status. `open` = observed in its last-seen scan,
 * not resolved by a later scan. `resolved` = a later scan covered the
 * service and the finding no longer appeared. */
export type FindingStatus = "open" | "resolved";

/** One aggregated finding row, ready to render. The `finding_id` is a
 * SHA-256 of `aws_account_id:rule_key` and is stable across scans;
 * `rule_key` is the scanner slug (e.g. `iam-user-no-mfa`) and is what
 * we pass to the knowledge-base lookups. */
export type Finding = {
  finding_id: string;
  aws_account_id: string;
  rule_key: string;
  raw_type: string;
  service: string;
  severity: Severity;
  description: string;
  rationale: string | null;
  dashboard_name: string | null;
  resource_path_pattern: string | null;
  checked_items: number;
  flagged_items: number;
  status: FindingStatus;
  first_seen_at: string;
  last_seen_at: string;
  first_seen_scan_id: string;
  last_seen_scan_id: string;
  resolved_at: string | null;
  resolved_in_scan_id: string | null;
};

export type FindingResource = {
  finding_id: string;
  aws_account_id: string;
  resource_path: string;
  invalid: boolean;
  first_seen_at: string;
  last_seen_at: string;
};

export type FindingDetail = {
  finding: Finding;
  resources: FindingResource[];
};

export type FindingsFilter = {
  severity?: Severity[];
  service?: string | null;
  status?: FindingStatus | null;
  limit?: number | null;
  offset?: number | null;
};

// --- Knowledge base & compliance mapping (Contract 08) --------------------

export type KnowledgeSource = "bundled" | "remote";

/** One knowledge-base article. The body fields are RAW markdown strings —
 * the frontend MUST render them through the sanitized markdown component
 * (no `dangerouslySetInnerHTML`) per Contract 09 §Constraints. */
export type KnowledgeArticle = {
  finding_id: string;
  matched: boolean;
  source: KnowledgeSource;
  title: string;
  description: string;
  risk: string;
  detection_logic: string;
  remediation: string;
  terraform_fix: string;
  aws_cli_fix: string;
  false_positives: string;
  unmatched_sections: Record<string, string>;
};

export type ControlReference = {
  control_id: string;
  title: string;
};

/** finding_id → framework_id → list of control entries. Frameworks the
 * finding has no entries in are omitted from the map. */
export type ControlMapping = {
  finding_id: string;
  frameworks: Record<string, ControlReference[]>;
};

export type Framework = {
  id: string;
  name: string;
};

// --- Scheduled & automated scans (Contract 10) ----------------------------

/** Cadence shape, mirrors the Rust `ScheduleCadence` enum. The frontend
 * picks one variant per schedule; `time_of_day_minutes` is required for
 * daily/weekly/monthly and ignored for interval. */
export type ScheduleCadence =
  | { kind: "daily" }
  | { kind: "weekly"; day_of_week: number }
  | { kind: "monthly"; day_of_month: number }
  | { kind: "interval"; minutes: number };

/** What the runner most recently did for a schedule. */
export type LastRunOutcome =
  | "fired"
  | "skipped_already_running"
  | "skipped_role_not_provisioned"
  | "skipped_scanner_unavailable"
  | "catch_up"
  | "skipped_internal_error";

/** One configured schedule. `next_run_at` is null when the schedule is
 * disabled or has no upcoming slot. */
export type Schedule = {
  aws_account_id: string;
  cadence: ScheduleCadence;
  time_of_day_minutes: number | null;
  enabled: boolean;
  last_run_at: string | null;
  last_run_outcome: LastRunOutcome | null;
  last_run_scan_id: string | null;
  next_run_at: string | null;
  created_at: string;
  updated_at: string;
};

export type SetScheduleInput = {
  aws_account_id: string;
  cadence: ScheduleCadence;
  time_of_day_minutes: number | null;
  enabled: boolean;
};

export type NextRunTime = {
  aws_account_id: string;
  next_run_at: string | null;
};

export type ScheduleEventKind =
  | "config_set"
  | "config_cleared"
  | "enabled"
  | "disabled"
  | "fired"
  | "skipped"
  | "catch_up";

export type ScheduleEvent = {
  event_id: string;
  aws_account_id: string;
  occurred_at: string;
  kind: ScheduleEventKind;
  reason: string | null;
  scan_id: string | null;
};

// --- Event log, retention, hard delete & panic (Contract 11) -------------

/** Stable, enumerated event kinds. Mirrors the Rust `EventKind` enum. The
 * frontend MUST treat any unknown string as a forward-compatible new kind
 * (don't `as never`-cast). */
export type EventKind =
  | "app_started"
  | "app_stopping"
  | "scan_completed"
  | "scan_failed"
  | "scan_canceled"
  | "scheduled_scan_fired"
  | "scheduled_scan_skipped"
  | "github_ticket_created"
  | "master_password_changed"
  | "master_password_reset"
  | "account_added"
  | "account_removed"
  | "scan_deleted"
  | "export"
  | "panic_wipe"
  | "settings_changed"
  | "retention_purged";

/** One row of the activity log. `aws_account_id_masked` is `****dddd` — the
 * backend never returns the full account ID over IPC in event-log payloads. */
export type EventLogEntry = {
  event_id: string;
  occurred_at: string;
  kind: EventKind;
  summary: string;
  detail: string | null;
  aws_account_id_masked: string | null;
  scan_id: string | null;
  path: string | null;
  item_count: number | null;
};

export type EventLogFilter = {
  kinds?: EventKind[];
  since?: string | null;
  until?: string | null;
  limit?: number | null;
  offset?: number | null;
  /** When true, ignore the "view cleared at" marker. Used for Export. */
  include_cleared?: boolean;
};

/** Retention period. `never` means "never auto-purge". */
export type RetentionPeriod =
  | { kind: "days"; days: number }
  | { kind: "never" };

export type RetentionSettings = {
  scan_retention: RetentionPeriod;
  eventlog_retention: RetentionPeriod;
  last_run_at: string | null;
};

export type RetentionRunSummary = {
  scan_dirs_removed: number;
  raw_files_removed: number;
  eventlog_rows_removed: number;
  scan_cutoff: string | null;
  eventlog_cutoff: string | null;
};

export type HardDeleteOptions = {
  secure_overwrite?: boolean;
};

export type HardDeleteSummary = {
  scan_id: string;
  findings_removed: number;
  findings_updated: number;
  resources_removed: number;
  raw_files_removed: number;
  raw_dir_removed: boolean;
  secure_overwrite_attempted: boolean;
  vacuum_run: boolean;
};

export type KeychainWipeResult = {
  removed: number;
  not_present: number;
  failed: number;
};

export type PanicWipeResult = {
  data_root_removed: boolean;
  db_files_removed: number;
  scan_dirs_removed: number;
  tf_workdirs_removed: number;
  log_files_removed: number;
  event_log_rows_wiped: number;
  keychain: KeychainWipeResult;
  self_delete_staged: boolean;
};

// --- GitHub integration (Contract 12) ------------------------------------

export type RepoSelection = { owner: string; name: string };

export type TokenStatus = { configured: boolean };

export type GithubSettings = {
  token: TokenStatus;
  findings_repo: RepoSelection | null;
  /** Hard-coded by the backend — the CloudSaw repo error reports land on. */
  error_report_repo: RepoSelection;
  /** `security@cloud-saw.com`. Backend-supplied so the UI shows the canonical
   * value rather than a hard-coded literal in the frontend. */
  security_contact: string;
};

export type DiagnosticBundle = {
  app_version: string;
  os_family: string;
  os_release: string;
  locale: string;
  generated_at: string;
  redacted_log_lines: string[];
  notes: string | null;
};

/** The exact content that would be submitted. Shown to the user BEFORE any
 * direct API call (Contract 12 §Constraints). The UI passes the same value
 * back to `githubSubmit*` so what the user reviewed is what gets sent. */
export type IssuePreview = {
  repo: RepoSelection;
  title: string;
  body: string;
  labels: string[];
  bundle: DiagnosticBundle;
};

export type IssueCreated = {
  repo: RepoSelection;
  issue_number: number;
  issue_url: string;
};

export type BrowserSubmission = { url: string };

export type FindingTicket = {
  finding_id: string;
  aws_account_id_masked: string;
  repo: RepoSelection;
  issue_number: number;
  issue_url: string;
  created_at: string;
};

// --- AI Suggestion Layer (Contract 13) -----------------------------------

export type AiProvider = "anthropic" | "openai";

export type EnvironmentType =
  | "production"
  | "dev_test"
  | "mixed"
  | "unspecified";

export type RiskTolerance = "low" | "medium" | "high" | "unspecified";

export type TeamSize = "solo" | "small" | "medium" | "large" | "unspecified";

export type BusinessContext = {
  industry: string;
  environment_type: EnvironmentType;
  compliance: string[];
  risk_tolerance: RiskTolerance;
  team_size: TeamSize;
};

export type ContextFlags = {
  industry_identifying: boolean;
  compliance_identifying: boolean;
};

export type AiSettings = {
  provider: AiProvider | null;
  key_connected: boolean;
  context: BusinessContext;
  flags: ContextFlags;
};

export type FindingDigest = {
  rule_key: string;
  service: string;
  severity: string;
  checked_items: number;
  flagged_items: number;
  resource_category: string;
};

/** The exact payload that will be transmitted. Shown to the user BEFORE
 * any send (Contract 13 §Constraints). The UI passes the same value to
 * `aiSendRequest` so what the user saw IS what gets sent. */
export type AiRequestPreview = {
  provider: AiProvider;
  model: string;
  system_prompt: string;
  user_message: string;
  digest: FindingDigest;
  context: BusinessContext;
  flags: ContextFlags;
  placeholders_used: string[];
};

export type AiSuggestion = {
  provider: AiProvider;
  model: string;
  generated_at: string;
  suggestion_markdown: string;
  usage_input_tokens: number | null;
  usage_output_tokens: number | null;
};

// --- Onboarding wizard (Contract 14) ----------------------------------

export type OnboardingStep =
  | "language"
  | "master_password"
  | "aws_account"
  | "terraform"
  | "business_context"
  | "first_scan"
  | "done";

export type OnboardingState = {
  completed: boolean;
  current_step: OnboardingStep;
  language: string;
  step_language_completed: boolean;
  step_password_completed: boolean;
  step_account_completed: boolean;
  step_terraform_completed: boolean;
  step_context_completed: boolean;
  step_first_scan_completed: boolean;
  completed_at: string | null;
};

// --- Report exporter (Contract 15) ------------------------------------

export type AccountIdDisclosure = "masked" | "full";

export type ExportOutcome = {
  primary_path: string;
  bytes_written: number;
  auto_export_path: string | null;
  auto_export_failed: boolean;
};

export type ReportSettings = {
  auto_export_enabled: boolean;
  auto_export_folder: string | null;
  mask_account_ids_default: boolean;
};

export const ipc = {
  /** CalVer build string, e.g. "2026.5.0". */
  appVersion(): Promise<string> {
    return invoke<string>("app_version");
  },

  // --- App lock ----------------------------------------------------------

  applockGetState(): Promise<LockState> {
    return invoke<LockState>("applock_get_state");
  },

  applockSetMasterPassword(password: string): Promise<void> {
    return invoke<void>("applock_set_master_password", { password });
  },

  applockUnlock(password: string): Promise<void> {
    return invoke<void>("applock_unlock", { password });
  },

  applockUnlockWithBiometric(reason: string): Promise<void> {
    return invoke<void>("applock_unlock_with_biometric", { reason });
  },

  applockLock(): Promise<void> {
    return invoke<void>("applock_lock");
  },

  applockChangePassword(oldPassword: string, newPassword: string): Promise<void> {
    return invoke<void>("applock_change_password", {
      oldPassword,
      newPassword,
    });
  },

  applockRecoveryUnlock(newPassword: string, reason: string): Promise<void> {
    return invoke<void>("applock_recovery_unlock", { newPassword, reason });
  },

  applockGetSettings(): Promise<LockSettings> {
    return invoke<LockSettings>("applock_get_settings");
  },

  applockSetSettings(settings: LockSettings): Promise<void> {
    return invoke<void>("applock_set_settings", { settings });
  },

  applockVerifyPassword(password: string): Promise<boolean> {
    return invoke<boolean>("applock_verify_password", { password });
  },

  // --- AWS auth --------------------------------------------------------

  authListProfiles(): Promise<ProfileInfo[]> {
    return invoke<ProfileInfo[]>("auth_list_profiles");
  },

  authGetCallerIdentity(profile: string): Promise<CallerIdentity> {
    return invoke<CallerIdentity>("auth_get_caller_identity", { profile });
  },

  authTestProfile(profile: string): Promise<ProfileTestResult> {
    return invoke<ProfileTestResult>("auth_test_profile", { profile });
  },

  // --- Multi-account ----------------------------------------------------

  accountsList(): Promise<Account[]> {
    return invoke<Account[]>("accounts_list");
  },

  accountsGet(awsAccountId: string): Promise<Account> {
    return invoke<Account>("accounts_get", { awsAccountId });
  },

  accountsAdd(input: AddAccountInput): Promise<Account> {
    return invoke<Account>("accounts_add", { input });
  },

  accountsUpdate(input: UpdateAccountInput): Promise<Account> {
    return invoke<Account>("accounts_update", { input });
  },

  accountsRemove(awsAccountId: string): Promise<RemovalImpact> {
    return invoke<RemovalImpact>("accounts_remove", { awsAccountId });
  },

  accountsGetActive(): Promise<string | null> {
    return invoke<string | null>("accounts_get_active");
  },

  accountsSetActive(awsAccountId: string | null): Promise<void> {
    return invoke<void>("accounts_set_active", { awsAccountId });
  },

  accountsGetDisplaySettings(): Promise<AccountsDisplaySettings> {
    return invoke<AccountsDisplaySettings>("accounts_get_display_settings");
  },

  accountsSetDisplaySettings(
    settings: AccountsDisplaySettings,
  ): Promise<void> {
    return invoke<void>("accounts_set_display_settings", { settings });
  },

  // --- Scanner-role connect flow ----------------------------------------

  /** Live values the "Create your role" recipes need: the caller ARN to put
   * in the trust policy, and the per-account external_id the user MUST use
   * in the `sts:ExternalId` condition. Calls `sts:GetCallerIdentity` on the
   * account's profile. */
  scannerRoleRequirements(awsAccountId: string): Promise<RoleRequirements> {
    return invoke<RoleRequirements>("scanner_role_requirements", {
      awsAccountId,
    });
  },

  /** Validate + record an externally-provisioned scanner role. Does a dry-run
   * `sts:AssumeRole` against the supplied ARN with the per-account external_id;
   * persists the role on success, surfaces typed errors on failure
   * (`scanner_role_assume_denied`, `scanner_role_not_found`, etc.). */
  scannerRoleConnect(
    awsAccountId: string,
    roleArn: string,
    policyVariant: PolicyVariant,
  ): Promise<ConnectResult> {
    return invoke<ConnectResult>("scanner_role_connect", {
      awsAccountId,
      roleArn,
      policyVariant,
    });
  },

  /** Report the per-account scanner-role state. Pure SQLite read. */
  scannerRoleStatus(awsAccountId: string): Promise<ProvisioningStatus> {
    return invoke<ProvisioningStatus>("scanner_role_status", {
      awsAccountId,
    });
  },

  // --- Scanner orchestrator --------------------------------------------

  /** Detect whether a bundled ScoutSuite binary is present AND passes its
   * SHA-256 integrity check. Pure local-state — no AWS calls. */
  scannerDetect(): Promise<ScoutSuiteAvailability> {
    return invoke<ScoutSuiteAvailability>("scanner_detect");
  },

  /** Start a scan for the given account. Returns the initial scan record
   * (already in `pending` or `assuming_role`); the frontend polls
   * `scannerScanStatus` for progress. */
  scannerRunScan(awsAccountId: string): Promise<ScanRecord> {
    return invoke<ScanRecord>("scanner_run_scan", { awsAccountId });
  },

  /** Poll a running scan's current state. */
  scannerScanStatus(scanId: string): Promise<ScanRecord> {
    return invoke<ScanRecord>("scanner_scan_status", { scanId });
  },

  /** Cancel a running scan. Idempotent — returns the current (terminal)
   * record if the scan is already finished. */
  scannerCancelScan(scanId: string): Promise<ScanRecord> {
    return invoke<ScanRecord>("scanner_cancel_scan", { scanId });
  },

  /** Open the OS file manager at the scan's output directory. Lets the
   * user inspect `raw-scout.json`, `scoutsuite-stderr.log`, and the
   * `scoutsuite-results/` tree without having to remember the per-OS
   * path (especially `~/Library/Application Support/CloudSaw/...` on
   * macOS, which Finder hides by default). Resolves on successful
   * spawn — there's no useful return value. */
  scannerRevealScanDir(scanId: string): Promise<void> {
    return invoke<void>("scanner_reveal_scan_dir", { scanId });
  },

  /** Most-recent scans for an account, newest first. */
  scannerListRecent(
    awsAccountId: string,
    limit?: number,
  ): Promise<ScanRecord[]> {
    return invoke<ScanRecord[]>("scanner_list_recent", {
      awsAccountId,
      limit: limit ?? null,
    });
  },

  // --- Findings parser & store ----------------------------------------

  /** Findings observed in a single scan, with optional severity / service
   * / status filtering. Always partitioned by the scan's account_id. */
  findingsList(scanId: string, filter?: FindingsFilter): Promise<Finding[]> {
    return invoke<Finding[]>("findings_list", {
      scanId,
      filter: filter ?? null,
    });
  },

  /** Full finding detail (row + resources) by stable finding_id. */
  findingsGet(findingId: string): Promise<FindingDetail> {
    return invoke<FindingDetail>("findings_get", { findingId });
  },

  /** Every scan for an account, newest first. Drives the History tab. */
  findingsListScans(awsAccountId: string): Promise<ScanRecord[]> {
    return invoke<ScanRecord[]>("findings_list_scans", { awsAccountId });
  },

  /** A single scan record, looked up by scan_id. */
  findingsGetScan(scanId: string): Promise<ScanRecord> {
    return invoke<ScanRecord>("findings_get_scan", { scanId });
  },

  // --- Knowledge base & compliance mappings ---------------------------

  /** Knowledge-base article for a finding. A finding without a matching
   * article returns `{ matched: false }` plus default copy — never an
   * error. The body fields are raw markdown; the caller MUST render
   * through the sanitized markdown component. */
  kbGetArticle(findingId: string): Promise<KnowledgeArticle> {
    return invoke<KnowledgeArticle>("kb_get_article", { findingId });
  },

  /** Compliance control mappings for a finding across all frameworks.
   * A finding with no mappings returns an empty `frameworks` map — never
   * an error. */
  kbGetControlMappings(findingId: string): Promise<ControlMapping> {
    return invoke<ControlMapping>("kb_get_control_mappings", { findingId });
  },

  /** Supported compliance frameworks (id + display name). */
  kbListFrameworks(): Promise<Framework[]> {
    return invoke<Framework[]>("kb_list_frameworks");
  },

  // --- Scheduled & automated scans ------------------------------------

  /** Configure (or replace) the schedule for an account. The backend
   * validates inputs and persists the schedule; the background runner
   * picks up the change without an app restart. */
  schedulerSetSchedule(input: SetScheduleInput): Promise<Schedule> {
    return invoke<Schedule>("scheduler_set_schedule", { input });
  },

  /** Read the configured schedule for an account. Rejects with
   * `schedule_not_found` if none is configured. */
  schedulerGetSchedule(awsAccountId: string): Promise<Schedule> {
    return invoke<Schedule>("scheduler_get_schedule", { awsAccountId });
  },

  /** Remove a configured schedule. Rejects with `schedule_not_found` if
   * none exists. */
  schedulerClearSchedule(awsAccountId: string): Promise<void> {
    return invoke<void>("scheduler_clear_schedule", { awsAccountId });
  },

  /** All configured schedules, ordered by account ID. */
  schedulerListSchedules(): Promise<Schedule[]> {
    return invoke<Schedule[]>("scheduler_list_schedules");
  },

  /** Upcoming-run timestamps for every schedule. Disabled rows return
   * `next_run_at = null`. */
  schedulerNextRunTimes(): Promise<NextRunTime[]> {
    return invoke<NextRunTime[]>("scheduler_next_run_times");
  },

  /** The N most-recent scheduled-scan events for an account. */
  schedulerRecentEvents(
    awsAccountId: string,
    limit?: number,
  ): Promise<ScheduleEvent[]> {
    return invoke<ScheduleEvent[]>("scheduler_recent_events", {
      awsAccountId,
      limit: limit ?? null,
    });
  },

  // --- Event log, retention, hard delete & panic (Contract 11) -------

  /** Paginated activity log. The default list hides entries from before
   * the user's last "Clear all" call; pass `include_cleared: true` to
   * see everything. */
  eventlogList(filter?: EventLogFilter): Promise<EventLogEntry[]> {
    return invoke<EventLogEntry[]>("eventlog_list", {
      filter: filter ?? null,
    });
  },

  /** Substring search over the event log (`summary` and `detail`). */
  eventlogSearch(query: string, limit?: number): Promise<EventLogEntry[]> {
    return invoke<EventLogEntry[]>("eventlog_search", {
      query,
      limit: limit ?? null,
    });
  },

  /** Returns the full activity log as newline-delimited JSON. */
  eventlogExport(): Promise<string> {
    return invoke<string>("eventlog_export");
  },

  /** Clear the activity-log VIEW. Underlying rows persist subject to
   * retention; Export still sees them. */
  eventlogClearView(): Promise<void> {
    return invoke<void>("eventlog_clear_view");
  },

  /** Total event-log row count. */
  eventlogCount(): Promise<number> {
    return invoke<number>("eventlog_count");
  },

  /** Read both retention periods + the last-run timestamp. */
  retentionGetSettings(): Promise<RetentionSettings> {
    return invoke<RetentionSettings>("retention_get_settings");
  },

  retentionSetScan(period: RetentionPeriod): Promise<void> {
    return invoke<void>("retention_set_scan", { period });
  },

  retentionSetEventlog(period: RetentionPeriod): Promise<void> {
    return invoke<void>("retention_set_eventlog", { period });
  },

  retentionRunNow(): Promise<RetentionRunSummary> {
    return invoke<RetentionRunSummary>("retention_run_now");
  },

  /** Hard-delete a scan. `confirmation` must be either the literal
   * `"DELETE"` or the full scan ID — the backend re-checks this. */
  deletionHardDeleteScan(
    scanId: string,
    confirmation: string,
    options?: HardDeleteOptions,
  ): Promise<HardDeleteSummary> {
    return invoke<HardDeleteSummary>("deletion_hard_delete_scan", {
      scanId,
      confirmation,
      options: options ?? null,
    });
  },

  /** Issue `VACUUM` against the SQLite file. */
  deletionVacuumNow(): Promise<void> {
    return invoke<void>("deletion_vacuum_now");
  },

  /** Wipe every CloudSaw trace on this machine. Requires the literal
   * confirmation string `"PANIC"`. Synchronous; returns once the data
   * wipe is done. */
  systemPanicWipe(confirmation: string): Promise<PanicWipeResult> {
    return invoke<PanicWipeResult>("system_panic_wipe", { confirmation });
  },

  /** User-level reboot. Called ONLY after the user explicitly picks
   * "Reboot now" in the post-panic dialog. */
  systemRequestReboot(): Promise<void> {
    return invoke<void>("system_request_reboot");
  },

  // --- GitHub integration (Contract 12) -------------------------------

  /** Read current PAT status, findings-ticket repo, error-report repo,
   * and security contact in one call. */
  githubGetSettings(): Promise<GithubSettings> {
    return invoke<GithubSettings>("github_get_settings");
  },

  /** Store the user-supplied PAT. The value goes ONLY to the OS keychain
   * — never SQLite, never logs, never URLs. */
  githubSetToken(token: string): Promise<void> {
    return invoke<void>("github_set_token", { token });
  },

  githubClearToken(): Promise<void> {
    return invoke<void>("github_clear_token");
  },

  githubSetFindingsRepo(repo: RepoSelection | null): Promise<void> {
    return invoke<void>("github_set_findings_repo", { repo });
  },

  /** URL of the GitHub fine-grained-token settings page. Frontend opens
   * this in the system browser when the user clicks "Generate token". */
  githubGenerateTokenUrl(): Promise<string> {
    return invoke<string>("github_generate_token_url");
  },

  /** Build the issue preview for an error report. The UI MUST show this
   * to the user before invoking `githubSubmitErrorReport`. */
  githubPrepareErrorReport(
    notes: string | null,
    locale: string,
  ): Promise<IssuePreview> {
    return invoke<IssuePreview>("github_prepare_error_report", { notes, locale });
  },

  /** Submit the error report via the GitHub API. Requires a configured
   * PAT; rejects with `github_no_token` otherwise. */
  githubSubmitErrorReport(preview: IssuePreview): Promise<IssueCreated> {
    return invoke<IssueCreated>("github_submit_error_report", { preview });
  },

  /** Build the prefilled GitHub new-issue URL for the browser fallback.
   * Always available — does NOT require a token. */
  githubBrowserFallbackForError(preview: IssuePreview): Promise<BrowserSubmission> {
    return invoke<BrowserSubmission>("github_browser_fallback_for_error", {
      preview,
    });
  },

  /** Build the issue preview for a finding ticket against a user-
   * selected repo. */
  githubPrepareFindingTicket(
    findingId: string,
    repo: RepoSelection,
  ): Promise<IssuePreview> {
    return invoke<IssuePreview>("github_prepare_finding_ticket", {
      findingId,
      repo,
    });
  },

  /** Submit the finding ticket via the GitHub API. Persists the
   * finding↔issue link on success. */
  githubSubmitFindingTicket(
    findingId: string,
    preview: IssuePreview,
  ): Promise<FindingTicket> {
    return invoke<FindingTicket>("github_submit_finding_ticket", {
      findingId,
      preview,
    });
  },

  githubBrowserFallbackForFinding(
    preview: IssuePreview,
  ): Promise<BrowserSubmission> {
    return invoke<BrowserSubmission>("github_browser_fallback_for_finding", {
      preview,
    });
  },

  /** Read the linked ticket for a finding, if any. */
  githubGetFindingTicket(findingId: string): Promise<FindingTicket | null> {
    return invoke<FindingTicket | null>("github_get_finding_ticket", { findingId });
  },

  /** All linked tickets for an account, newest first. */
  githubListFindingTickets(awsAccountId: string): Promise<FindingTicket[]> {
    return invoke<FindingTicket[]>("github_list_finding_tickets", {
      awsAccountId,
    });
  },

  // --- AI Suggestion Layer (Contract 13) ------------------------------

  /** Read provider selection, key status, business context, and the
   * "this looks identifying" flags in one round-trip. */
  aiGetSettings(): Promise<AiSettings> {
    return invoke<AiSettings>("ai_get_settings");
  },

  aiSetProvider(provider: AiProvider | null): Promise<void> {
    return invoke<void>("ai_set_provider", { provider });
  },

  /** Store the user-supplied API key. The value goes ONLY to the OS
   * keychain — never SQLite, never logs, never URLs. */
  aiSetProviderKey(provider: AiProvider, key: string): Promise<void> {
    return invoke<void>("ai_set_provider_key", { provider, key });
  },

  aiClearProviderKey(provider: AiProvider): Promise<void> {
    return invoke<void>("ai_clear_provider_key", { provider });
  },

  aiHasProviderKey(provider: AiProvider): Promise<boolean> {
    return invoke<boolean>("ai_has_provider_key", { provider });
  },

  aiSetBusinessContext(context: BusinessContext): Promise<void> {
    return invoke<void>("ai_set_business_context", { context });
  },

  /** Build the request preview the UI MUST show to the user. The
   * returned value IS the payload that would be transmitted. */
  aiPrepareRequest(findingId: string): Promise<AiRequestPreview> {
    return invoke<AiRequestPreview>("ai_prepare_request", { findingId });
  },

  /** Send the previously-built preview to the connected provider.
   * Rejects with `ai_no_provider_key` if no key is connected. */
  aiSendRequest(preview: AiRequestPreview): Promise<AiSuggestion> {
    return invoke<AiSuggestion>("ai_send_request", { preview });
  },

  // --- Onboarding wizard (Contract 14) -------------------------------

  /** Read the full wizard state. App.tsx calls this on mount; if
   * `completed` is false, the only entry point is the wizard. */
  onboardingGetState(): Promise<OnboardingState> {
    return invoke<OnboardingState>("onboarding_get_state");
  },

  onboardingSetLanguage(language: string): Promise<void> {
    return invoke<void>("onboarding_set_language", { language });
  },

  onboardingSetCurrentStep(step: OnboardingStep): Promise<void> {
    return invoke<void>("onboarding_set_current_step", { step });
  },

  onboardingMarkStepCompleted(step: OnboardingStep): Promise<void> {
    return invoke<void>("onboarding_mark_step_completed", { step });
  },

  /** Flip the global completed flag. Called after the FirstScan step
   * finishes. Subsequent launches route straight to the main app. */
  onboardingComplete(): Promise<void> {
    return invoke<void>("onboarding_complete");
  },

  /** Re-enter the wizard from Settings, jumping straight to a
   * specific step (typically `aws_account` to add another). */
  onboardingResetForRerun(startAt: OnboardingStep): Promise<void> {
    return invoke<void>("onboarding_reset_for_rerun", { startAt });
  },

  // --- Report exporter (Contract 15) ---------------------------------

  /** Per-scan HTML export. The `outputPath` MUST come from the native
   * save dialog. */
  reportExportScanHtml(
    scanId: string,
    outputPath: string,
    disclosure: AccountIdDisclosure,
  ): Promise<ExportOutcome> {
    return invoke<ExportOutcome>("report_export_scan_html", {
      scanId,
      outputPath,
      disclosure,
    });
  },

  reportExportScanPdf(
    scanId: string,
    outputPath: string,
    disclosure: AccountIdDisclosure,
  ): Promise<ExportOutcome> {
    return invoke<ExportOutcome>("report_export_scan_pdf", {
      scanId,
      outputPath,
      disclosure,
    });
  },

  reportExportCustomHtml(
    start: string,
    end: string,
    accountScope: string[],
    outputPath: string,
    disclosure: AccountIdDisclosure,
  ): Promise<ExportOutcome> {
    return invoke<ExportOutcome>("report_export_custom_html", {
      start,
      end,
      accountScope,
      outputPath,
      disclosure,
    });
  },

  reportExportCustomPdf(
    start: string,
    end: string,
    accountScope: string[],
    outputPath: string,
    disclosure: AccountIdDisclosure,
  ): Promise<ExportOutcome> {
    return invoke<ExportOutcome>("report_export_custom_pdf", {
      start,
      end,
      accountScope,
      outputPath,
      disclosure,
    });
  },

  reportGetSettings(): Promise<ReportSettings> {
    return invoke<ReportSettings>("report_get_settings");
  },

  reportSetSettings(settings: ReportSettings): Promise<void> {
    return invoke<void>("report_set_settings", { settings });
  },
};

export type Ipc = typeof ipc;
