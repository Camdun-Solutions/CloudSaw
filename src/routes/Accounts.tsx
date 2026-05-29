// Accounts — multi-account configuration & management (Contract 04).
//
// This is the CloudSaw-local concept of an "account": a user label tied to an
// AWS profile, with the AWS account ID verified at save-time via
// `sts:GetCallerIdentity`. The Profiles page (Contract 03) is the underlying
// diagnostics view of `~/.aws/config`; this page wraps profiles into named,
// stored accounts and tracks which one is the active partitioning key for
// every account-scoped query elsewhere in the app.
//
// The screen never sees credential material. Account IDs are masked by
// default per Contract 04 §Constraints; the Settings toggle persists in the
// backend (settings table) and is read on mount.

import { useCallback, useEffect, useMemo, useState } from "react";

import { Badge, Button, EmptyState, Logo, MeatballMenu, type MeatballMenuItem, Modal, Select } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  maskAccountId,
  type Account,
  type AccountsDisplaySettings,
  type AddAccountInput,
  type Environment,
  type ProfileInfo,
  type ProfileTestResult,
  type RemovalImpact,
  type UpdateAccountInput,
} from "@/lib/ipc";

/** PR #66: per-profile inline test state. The AWS CLI Profiles
 *  section renders a small indicator next to each profile's Test
 *  button — running spinner, OK check, or Failed X. */
type ProfileTestState =
  | { phase: "running" }
  | { phase: "ok" }
  | { phase: "fail"; code: string };
import ConnectScannerRoleForm from "@/components/ConnectScannerRoleForm";
import { SCAN_FINISHED_EVENT } from "@/contexts/ScanModalContext";
import ScanProgressModal from "@/routes/ScanProgress";

type Props = {
  /** Standalone-route close handler. Required unless `embedded` is
   *  true (embedded mode is owned by the host Settings page and
   *  has no separate close affordance). */
  onClose?: () => void;
  /**
   * PR #46: render as an inline section inside the Settings page
   * rather than as a standalone route. When true:
   *   - The outer <main> wrapper + the page-level header (logo,
   *     title, refresh + close buttons) are skipped.
   *   - The "Open profiles" + "Refresh" buttons reflow into a
   *     compact action row above the configured-accounts list.
   *   - The "Close" button is hidden — the user navigates away
   *     via the persistent TopNav (PR #41) or by scrolling
   *     within Settings to another section.
   *
   * Default false preserves the legacy standalone-route shape so
   * any not-yet-migrated caller keeps rendering correctly. The
   * App.tsx route handler for "accounts" was removed in the same
   * PR though, so embedded=true is effectively the only call
   * site post-merge.
   */
  embedded?: boolean;
};

const ENVIRONMENTS: Environment[] = ["dev", "staging", "prod", "other"];

export default function Accounts({
  embedded = false,
}: Props) {
  const t = useT();
  const formatError = useIpcError();

  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  // Names of profiles currently in `~/.aws/config`. Used to flag accounts
  // whose profile was deleted out-of-band (Contract 04 §Edge Cases) AND
  // to populate the new AWS CLI Profiles section at the bottom of the
  // page (PR #66).
  const [presentProfiles, setPresentProfiles] = useState<Set<string>>(new Set());
  // PR #66: map of aws_account_id → role_arn for accounts whose scanner
  // role has been provisioned. Populated by parallel `scannerRoleStatus`
  // calls during reload(). The AccountRow displays the role name parsed
  // from this ARN next to the "Scanner role" label.
  const [roleArnByAccount, setRoleArnByAccount] = useState<Record<string, string>>({});
  // PR #66: full ProfileInfo list — used by the new AWS CLI Profiles
  // section. `presentProfiles` is a Set<string> derived from this; the
  // section needs the full ProfileInfo (with `source`) to badge SSO
  // profiles.
  const [allProfiles, setAllProfiles] = useState<ProfileInfo[]>([]);
  // PR #66: per-profile test state. Keyed by profile name. Either
  // running, or terminal with a result tag the row renders as a small
  // inline indicator next to the Test button.
  const [profileTests, setProfileTests] = useState<
    Record<string, ProfileTestState>
  >({});
  const [addProfileOpen, setAddProfileOpen] = useState(false);
  // PR #66: top-of-section toast banner. Set by AddProfileModal on
  // success / error so the user sees the outcome without dismissing
  // the modal. Auto-cleared after ~4s for success; user-dismissable
  // for errors.
  const [toast, setToast] = useState<{ kind: "success" | "error"; msg: string } | null>(null);
  const [display, setDisplay] = useState<AccountsDisplaySettings>({
    reveal_full_ids: false,
  });
  const [loadError, setLoadError] = useState<string | null>(null);

  // Modal state. The "edit" modal accepts the row to edit; the "remove" modal
  // accepts the row to remove. Add modal is the simpler boolean form. The
  // provisioning modal (Contract 05) is per-row, like edit/remove.
  const [addOpen, setAddOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<Account | null>(null);
  const [removeTarget, setRemoveTarget] = useState<Account | null>(null);
  const [provisionTarget, setProvisionTarget] = useState<Account | null>(null);
  const [scanTarget, setScanTarget] = useState<Account | null>(null);

  const reload = useCallback(async () => {
    setLoadError(null);
    try {
      const [list, active, settings, profiles] = await Promise.all([
        ipc.accountsList(),
        ipc.accountsGetActive(),
        ipc.accountsGetDisplaySettings(),
        ipc.authListProfiles(),
      ]);
      setAccounts(list);
      setActiveId(active);
      setDisplay(settings);
      setPresentProfiles(new Set(profiles.map((p) => p.name)));
      setAllProfiles(profiles);
      // PR #66: fetch the scanner-role provisioning status for every
      // account in parallel. We only need the role_arn for display;
      // a missing/unprovisioned/failed status leaves the entry out of
      // the map and the row falls back to the boolean status label.
      const provisioned = list.filter((a) => a.role_provisioned);
      const arnPairs = await Promise.all(
        provisioned.map(async (a) => {
          try {
            const status = await ipc.scannerRoleStatus(a.aws_account_id);
            return status.status === "provisioned"
              ? ([a.aws_account_id, status.role_arn] as const)
              : null;
          } catch {
            return null;
          }
        }),
      );
      const arnMap: Record<string, string> = {};
      for (const pair of arnPairs) {
        if (pair) arnMap[pair[0]] = pair[1];
      }
      setRoleArnByAccount(arnMap);
    } catch (err) {
      setLoadError(formatError(err));
      setAccounts([]);
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function onSetActive(awsAccountId: string | null) {
    try {
      await ipc.accountsSetActive(awsAccountId);
      setActiveId(awsAccountId);
    } catch (err) {
      setLoadError(formatError(err));
    }
  }

  async function onToggleReveal(next: boolean) {
    const previous = display.reveal_full_ids;
    setDisplay({ reveal_full_ids: next });
    try {
      await ipc.accountsSetDisplaySettings({ reveal_full_ids: next });
    } catch (err) {
      setDisplay({ reveal_full_ids: previous });
      setLoadError(formatError(err));
    }
  }

  // PR #66: trigger an inline AWS CLI profile test. Result tag drives
  // the inline indicator next to the row's Test button.
  const runProfileTest = useCallback(
    async (profileName: string) => {
      setProfileTests((prev) => ({
        ...prev,
        [profileName]: { phase: "running" },
      }));
      try {
        const result: ProfileTestResult = await ipc.authTestProfile(profileName);
        if (result.status === "success") {
          setProfileTests((prev) => ({
            ...prev,
            [profileName]: { phase: "ok" },
          }));
        } else {
          setProfileTests((prev) => ({
            ...prev,
            [profileName]: { phase: "fail", code: result.reason },
          }));
        }
      } catch (err) {
        setProfileTests((prev) => ({
          ...prev,
          [profileName]: { phase: "fail", code: formatError(err) },
        }));
      }
    },
    [formatError],
  );

  // PR #66: auto-dismiss success toasts so the banner doesn't linger
  // forever. Error toasts persist until the user dismisses them or a
  // subsequent success replaces them.
  useEffect(() => {
    if (!toast || toast.kind !== "success") return;
    const handle = window.setTimeout(() => setToast(null), 4000);
    return () => window.clearTimeout(handle);
  }, [toast]);

  // Standalone-mode header. PR #66: the "Diagnose profiles" +
  // "Refresh" + "Close" action cluster is gone — Diagnose profiles
  // was replaced by the inline AWS CLI Profiles section below, the
  // refresh button was redundant (reload happens on mount + after
  // every mutation), and standalone mode is unreachable from the
  // router today anyway. The header keeps the logo + title so any
  // future direct-render usage still has a recognizable anchor.
  // PR #75: sticky-top page header so the title bar stays visible
  // while body content scrolls underneath. z-20 sits below the
  // floating TopNav chip (z-30). Applies to the standalone-route
  // render path; when Accounts is embedded as a Settings panel the
  // outer Settings header already provides the sticky bar.
  const standaloneHeader = (
    <header className="sticky top-0 z-20 border-b border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-8 py-5">
      <div className="flex items-center gap-3">
        <Logo size="sm" />
        <div className="flex flex-col">
          <h1 className="text-h2 font-semibold tracking-tight">
            {t("accounts.title")}
          </h1>
          <p className="text-small text-saw-grey-500 dark:text-saw-grey-400">
            {t("accounts.subtitle")}
          </p>
        </div>
      </div>
    </header>
  );

  // Body content — identical in both modes. Section padding/max-
  // width is dropped in embedded mode because the host Settings
  // page already imposes its own layout container.
  const body = (
    <>
      <section
        className={
          embedded
            ? "flex flex-col gap-3"
            : "mx-auto max-w-4xl px-8 py-10"
        }
      >
        {/* PR #66: top-of-section toast banner. Add-profile success
            lands here and auto-dismisses after 4s; errors persist
            until the user dismisses them. */}
        {toast ? (
          <div
            role={toast.kind === "error" ? "alert" : "status"}
            className={
              "mb-4 flex items-start justify-between gap-3 rounded-card px-4 py-3 text-small " +
              (toast.kind === "success"
                ? "border border-emerald-200 bg-emerald-50 text-emerald-900 dark:border-emerald-800 dark:bg-emerald-900/30 dark:text-emerald-200"
                : "border border-saw-red/30 bg-saw-red/5 text-saw-red")
            }
            data-testid={`accounts-toast-${toast.kind}`}
          >
            <span className="flex-1">{toast.msg}</span>
            <button
              type="button"
              onClick={() => setToast(null)}
              aria-label="Dismiss"
              className="text-current opacity-70 hover:opacity-100"
              data-testid="accounts-toast-dismiss"
            >
              ✕
            </button>
          </div>
        ) : null}

        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-h3 font-semibold tracking-tight">
              {t("accounts.section.configured")}
            </h2>
            <p className="mt-1 max-w-2xl text-small text-saw-grey-600 dark:text-saw-grey-400">
              {t("accounts.section.configured_hint")}
            </p>
          </div>
          <Button
            variant="primary"
            onClick={() => setAddOpen(true)}
            data-testid="accounts-add-open"
          >
            {t("accounts.add.cta")}
          </Button>
        </div>

        <label className="mt-4 inline-flex items-center gap-2 text-small text-saw-grey-600 dark:text-saw-grey-400">
          <input
            type="checkbox"
            checked={display.reveal_full_ids}
            onChange={(e) => void onToggleReveal(e.target.checked)}
            data-testid="accounts-reveal-toggle"
            className="h-4 w-4 rounded border-saw-grey-300 dark:border-saw-grey-700"
          />
          {t("accounts.reveal_toggle.label")}
        </label>

        {loadError ? (
          <p
            role="alert"
            className="mt-6 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-4 py-3 text-body text-saw-red"
            data-testid="accounts-load-error"
          >
            {loadError}
          </p>
        ) : null}

        <div className="mt-6" data-testid="accounts-list">
          {accounts === null ? (
            <p className="text-body text-saw-grey-600 dark:text-saw-grey-400">{t("common.loading")}</p>
          ) : accounts.length === 0 ? (
            <EmptyState
              title={t("accounts.empty.title")}
              body={t("accounts.empty.body")}
              action={
                <Button
                  variant="primary"
                  onClick={() => setAddOpen(true)}
                  data-testid="accounts-empty-add"
                >
                  {t("accounts.add.cta")}
                </Button>
              }
            />
          ) : (
            <ul
              className="divide-y divide-saw-grey-200 dark:divide-saw-grey-700 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark"
              data-testid="accounts-rows"
            >
              {accounts.map((a) => (
                <AccountRow
                  key={a.aws_account_id}
                  account={a}
                  active={a.aws_account_id === activeId}
                  revealFullId={display.reveal_full_ids}
                  profileMissing={!presentProfiles.has(a.profile_name)}
                  roleName={parseRoleNameFromArn(
                    roleArnByAccount[a.aws_account_id],
                  )}
                  onSetActive={() => void onSetActive(a.aws_account_id)}
                  onEdit={() => setEditTarget(a)}
                  onRemove={() => setRemoveTarget(a)}
                  onProvision={() => setProvisionTarget(a)}
                  onScan={() => setScanTarget(a)}
                />
              ))}
            </ul>
          )}
        </div>
      </section>

      {/* PR #66: AWS CLI Profiles section. Lists every profile parsed
          from ~/.aws/config with a Test affordance. Replaces the
          deleted Profiles route + "Diagnose profiles" CTA. */}
      <section
        className={
          embedded
            ? "mt-6 flex flex-col gap-3"
            : "mx-auto max-w-4xl px-8 pb-10"
        }
        data-testid="accounts-cli-profiles-section"
      >
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-h3 font-semibold tracking-tight">
              {t("accounts.cli_profiles.title")}
            </h2>
            <p className="mt-1 max-w-2xl text-small text-saw-grey-600 dark:text-saw-grey-400">
              {t("accounts.cli_profiles.subtitle")}
            </p>
          </div>
          <Button
            variant="primary"
            onClick={() => setAddProfileOpen(true)}
            data-testid="accounts-cli-profiles-add"
          >
            {t("accounts.cli_profiles.add_cta")}
          </Button>
        </div>

        <div className="mt-4" data-testid="accounts-cli-profiles-list">
          {allProfiles.length === 0 ? (
            <p
              className="rounded-card border border-dashed border-saw-grey-200 dark:border-saw-grey-700 px-4 py-6 text-center text-small text-saw-grey-600 dark:text-saw-grey-400"
              data-testid="accounts-cli-profiles-empty"
            >
              {t("accounts.cli_profiles.empty")}
            </p>
          ) : (
            <ul
              className="divide-y divide-saw-grey-200 dark:divide-saw-grey-700 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark"
              data-testid="accounts-cli-profiles-rows"
            >
              {allProfiles.map((p) => (
                <ProfileRow
                  key={p.name}
                  profile={p}
                  testState={profileTests[p.name]}
                  onTest={() => void runProfileTest(p.name)}
                />
              ))}
            </ul>
          )}
        </div>
      </section>

      <AddAccountModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onAdded={async () => {
          setAddOpen(false);
          await reload();
        }}
      />

      <EditAccountModal
        target={editTarget}
        onClose={() => setEditTarget(null)}
        onSaved={async () => {
          setEditTarget(null);
          await reload();
        }}
      />

      <RemoveAccountModal
        target={removeTarget}
        revealFullId={display.reveal_full_ids}
        onClose={() => setRemoveTarget(null)}
        onRemoved={async (impact) => {
          setRemoveTarget(null);
          await reload();
          if (impact.was_active) {
            // The active selection was cleared in the same transaction; we
            // just synced state via reload(). No further action here — the
            // empty active strip prompts the user to choose a new one.
          }
        }}
      />

      {/* PR #66: Add an AWS CLI profile. Writes the supplied
          credentials directly to ~/.aws/credentials and the region +
          output to ~/.aws/config via the new `auth_create_profile`
          IPC. CloudSaw briefly holds the secret in memory to forward
          it to the IPC; it is not stored, logged, or transmitted. */}
      <AddProfileModal
        open={addProfileOpen}
        existingProfileNames={allProfiles.map((p) => p.name)}
        onClose={() => setAddProfileOpen(false)}
        onSaved={async (name) => {
          setAddProfileOpen(false);
          setToast({
            kind: "success",
            msg: t("accounts.cli_profiles.added_success").replace("{name}", name),
          });
          await reload();
        }}
        onError={(msg) => setToast({ kind: "error", msg })}
      />

      {/* Phase 2 — "Connect scanner role" modal. Wraps the shared
        * ConnectScannerRoleForm component so the per-account action on
        * this page surfaces the same flow as onboarding step 4.
        * PR #53: `size="lg"` so the recipe blocks (CFN YAML, Terraform
        * HCL) don't word-wrap into illegibility, and the modal's body
        * is now scrollable so long content (which previously
        * overflowed the viewport) is reachable. */}
      {provisionTarget ? (
        <Modal
          open={true}
          onClose={() => setProvisionTarget(null)}
          title={"Connect scanner role"}
          size="lg"
          footer={
            <Button
              variant="ghost"
              onClick={() => setProvisionTarget(null)}
              data-testid="accounts-connect-role-close"
            >
              Close
            </Button>
          }
        >
          <ConnectScannerRoleForm
            awsAccountId={provisionTarget.aws_account_id}
            onConnected={async () => {
              await reload();
            }}
          />
        </Modal>
      ) : null}

      {/* Conditionally mount so the modal's new account-picker phase
          (PR #39 — when the account prop is null) doesn't fire here.
          Accounts.tsx is the legacy pre-bound caller — the modal must
          only render when a row's Scan button was clicked. */}
      {scanTarget ? (
        <ScanProgressModal
          account={scanTarget}
          onClose={() => setScanTarget(null)}
          onScanFinished={async () => {
            // Refresh accounts so the last_scan_at / last_scan_status
            // badges pick up the new terminal state.
            await reload();
            // PR #54: also dispatch the global SCAN_FINISHED_EVENT
            // so the App.tsx listener fires the desktop
            // notification (when the user has opted in) and the
            // Dashboard Welcome page (PR #50) refreshes its
            // recent-activity / top-findings cards. The global
            // ScanModalProvider already dispatches this on its
            // own modal-driven scans; this is the legacy
            // pre-bound-modal callsite catching up.
            document.dispatchEvent(new CustomEvent(SCAN_FINISHED_EVENT));
          }}
        />
      ) : null}
    </>
  );

  // Embedded mode (PR #46): host Settings page owns the page-level
  // <main> + chrome, so we just return the body fragment. Standalone
  // mode wraps the same body in the original <main> + header.
  if (embedded) {
    return body;
  }
  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black text-saw-grey-900 dark:text-saw-beige">
      {standaloneHeader}
      {body}
    </main>
  );
}

function AccountRow({
  account,
  active,
  revealFullId,
  profileMissing,
  roleName,
  onSetActive,
  onEdit,
  onRemove,
  onProvision,
  onScan,
}: {
  account: Account;
  active: boolean;
  revealFullId: boolean;
  profileMissing: boolean;
  /** Parsed name segment of the provisioned scanner role's ARN,
   *  e.g. "CloudSawScanner". Undefined when the role isn't
   *  provisioned or the ARN lookup failed. */
  roleName: string | undefined;
  onSetActive: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onProvision: () => void;
  onScan: () => void;
}) {
  const t = useT();
  const displayedId = revealFullId
    ? account.aws_account_id
    : maskAccountId(account.aws_account_id);

  // PR #66: collapse the per-row action surface into a vertical
  // meatball menu anchored at the top-right of the card. Delete
  // sits last in saw-red. The Set-active affordance stays as a
  // separate ghost button below the dl because it is a status
  // flip, not a lifecycle action.
  const menuItems: MeatballMenuItem[] = [
    {
      label: t("accounts.row.edit"),
      onClick: onEdit,
      testId: `account-edit-${account.aws_account_id}`,
    },
    ...(account.role_provisioned
      ? [
          {
            label: t("scanner.scan.cta"),
            onClick: onScan,
            testId: `account-scan-${account.aws_account_id}`,
          },
        ]
      : []),
    {
      // PR #75: label tracks whether the role has ever been
      // provisioned for this account. First-time configuration reads
      // "Configure scanner role"; subsequent edits to an existing
      // role read "Re-configure scanner role". Same onClick path —
      // the underlying flow is identical (it always replans), the
      // distinction is purely the verb the user sees.
      label: account.role_provisioned
        ? t("terraform.provision.replan_cta")
        : t("terraform.provision.configure_cta"),
      onClick: onProvision,
      testId: `account-provision-${account.aws_account_id}`,
    },
    {
      label: t("accounts.row.remove"),
      onClick: onRemove,
      danger: true,
      testId: `account-remove-${account.aws_account_id}`,
    },
  ];

  return (
    <li className="px-5 py-4" data-testid={`account-row-${account.aws_account_id}`}>
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <p
              className="min-w-0 truncate text-body font-medium"
              data-testid={`account-label-${account.aws_account_id}`}
            >
              {account.label}
            </p>
            <Badge tone="neutral" data-testid={`account-env-${account.aws_account_id}`}>
              {t(`accounts.env.${account.environment}`)}
            </Badge>
            {/* PR #66: active dot was previously tone="info" (gold).
                Changed to tone="success" (emerald) per user spec. */}
            {active ? (
              <Badge
                tone="success"
                data-testid={`account-active-${account.aws_account_id}`}
              >
                {t("accounts.row.active_badge")}
              </Badge>
            ) : null}
            {profileMissing ? (
              <Badge
                tone="danger"
                data-testid={`account-invalid-${account.aws_account_id}`}
              >
                {t("accounts.row.profile_missing_badge")}
              </Badge>
            ) : null}
          </div>
          {profileMissing ? (
            <p
              role="alert"
              className="mt-2 rounded-card bg-saw-grey-50 dark:bg-saw-black px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
              data-testid={`account-invalid-hint-${account.aws_account_id}`}
            >
              {t("accounts.row.profile_missing_hint").replace(
                "{profile}",
                account.profile_name,
              )}
            </p>
          ) : null}
          <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-0.5 text-small text-saw-grey-600 dark:text-saw-grey-400">
            <dt className="text-saw-grey-500 dark:text-saw-grey-400">{t("accounts.row.profile")}</dt>
            <dd className="min-w-0 break-all font-mono">{account.profile_name}</dd>
            <dt className="text-saw-grey-500 dark:text-saw-grey-400">{t("accounts.row.account_id")}</dt>
            <dd
              className="min-w-0 break-all font-mono"
              data-testid={`account-id-${account.aws_account_id}`}
            >
              {displayedId}
            </dd>
            <dt className="text-saw-grey-500 dark:text-saw-grey-400">{t("accounts.row.role_status")}</dt>
            <dd
              className="min-w-0 break-all"
              data-testid={`account-role-status-${account.aws_account_id}`}
            >
              {account.role_provisioned ? (
                <>
                  {t("accounts.row.role_provisioned")}
                  {/* PR #66: show the role name parsed from the ARN
                      (e.g. "CloudSawScanner") alongside the status so
                      users can see which role is wired without
                      opening the provisioning modal. */}
                  {roleName ? (
                    <span className="ml-1 font-mono text-saw-grey-500 dark:text-saw-grey-400">
                      ({roleName})
                    </span>
                  ) : null}
                </>
              ) : (
                t("accounts.row.role_not_provisioned")
              )}
            </dd>
            <dt className="text-saw-grey-500 dark:text-saw-grey-400">{t("accounts.row.last_scan")}</dt>
            <dd>
              {account.last_scan_at
                ? formatTs(account.last_scan_at)
                : t("accounts.row.never_scanned")}
            </dd>
          </dl>
          {!active ? (
            <div className="mt-3">
              <Button
                variant="secondary"
                size="sm"
                onClick={onSetActive}
                data-testid={`account-set-active-${account.aws_account_id}`}
              >
                {t("accounts.row.set_active")}
              </Button>
            </div>
          ) : null}
        </div>
        <div className="flex-shrink-0">
          <MeatballMenu
            items={menuItems}
            triggerLabel={t("accounts.row.edit") + " / " + t("scanner.scan.cta")}
            triggerTestId={`account-menu-${account.aws_account_id}`}
          />
        </div>
      </div>
    </li>
  );
}

// --- Add modal ----------------------------------------------------------

function AddAccountModal({
  open,
  onClose,
  onAdded,
}: {
  open: boolean;
  onClose: () => void;
  onAdded: () => Promise<void>;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [label, setLabel] = useState("");
  const [profile, setProfile] = useState("");
  const [environment, setEnvironment] = useState<Environment>("dev");
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [profilesLoaded, setProfilesLoaded] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setLabel("");
      setProfile("");
      setEnvironment("dev");
      setError(null);
      setSubmitting(false);
      setProfilesLoaded(false);
      return;
    }
    void ipc
      .authListProfiles()
      .then((list) => {
        setProfiles(list);
        setProfilesLoaded(true);
        if (list.length > 0 && profile === "") {
          setProfile(list[0].name);
        }
      })
      .catch((err) => {
        setError(formatError(err));
        setProfilesLoaded(true);
      });
    // We intentionally exclude `profile` from deps — we only want to seed it
    // when the modal opens, not re-seed every keystroke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const canSubmit =
    !submitting &&
    label.trim().length > 0 &&
    profile.length > 0 &&
    profilesLoaded;

  async function onSubmit() {
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    const input: AddAccountInput = {
      label: label.trim(),
      profile_name: profile,
      environment,
    };
    try {
      await ipc.accountsAdd(input);
      await onAdded();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setSubmitting(false);
    }
  }

  const envOptions = useMemo(
    () =>
      ENVIRONMENTS.map((env) => ({
        value: env,
        label: t(`accounts.env.${env}`),
      })),
    [t],
  );

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t("accounts.add.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={submitting}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onSubmit}
            disabled={!canSubmit}
            data-testid="accounts-add-submit"
          >
            {submitting ? t("accounts.add.verifying") : t("accounts.add.submit")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <p className="text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("accounts.add.explainer")}
        </p>

        <label className="flex flex-col gap-1.5">
          <span className="text-small font-medium text-saw-grey-700 dark:text-saw-grey-300">
            {t("accounts.add.label_field")}
          </span>
          <input
            type="text"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            data-testid="accounts-add-label"
            maxLength={64}
            className="block w-full rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
          />
          <span className="text-small text-saw-grey-500 dark:text-saw-grey-400">
            {t("accounts.add.label_hint")}
          </span>
        </label>

        {profilesLoaded && profiles.length === 0 ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-50 dark:bg-saw-black px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="accounts-add-no-profiles"
          >
            {t("accounts.add.no_profiles")}
          </p>
        ) : (
          <Select<string>
            label={t("accounts.add.profile_field")}
            value={profile}
            options={profiles.map((p) => ({
              value: p.name,
              label:
                p.source === "sso"
                  ? `${p.name} — ${t("profiles.source.sso")}`
                  : `${p.name} — ${t("profiles.source.cli")}`,
            }))}
            onChange={setProfile}
            data-testid="accounts-add-profile"
            description={t("accounts.add.profile_hint")}
          />
        )}

        <Select<Environment>
          label={t("accounts.add.environment_field")}
          value={environment}
          options={envOptions}
          onChange={setEnvironment}
          data-testid="accounts-add-env"
        />

        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            data-testid="accounts-add-error"
          >
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}

// --- Edit modal --------------------------------------------------------

function EditAccountModal({
  target,
  onClose,
  onSaved,
}: {
  target: Account | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [label, setLabel] = useState("");
  const [profile, setProfile] = useState("");
  const [environment, setEnvironment] = useState<Environment>("dev");
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!target) {
      setError(null);
      setSubmitting(false);
      return;
    }
    setLabel(target.label);
    setProfile(target.profile_name);
    setEnvironment(target.environment);
    void ipc
      .authListProfiles()
      .then(setProfiles)
      .catch((err) => setError(formatError(err)));
  }, [target, formatError]);

  const canSubmit =
    !submitting &&
    label.trim().length > 0 &&
    profile.length > 0 &&
    target !== null;

  async function onSubmit() {
    if (!canSubmit || !target) return;
    setSubmitting(true);
    setError(null);
    const input: UpdateAccountInput = {
      aws_account_id: target.aws_account_id,
      label: label.trim(),
      profile_name: profile,
      environment,
    };
    try {
      await ipc.accountsUpdate(input);
      await onSaved();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setSubmitting(false);
    }
  }

  const envOptions = useMemo(
    () =>
      ENVIRONMENTS.map((env) => ({
        value: env,
        label: t(`accounts.env.${env}`),
      })),
    [t],
  );

  // Always-include the current profile name in the dropdown so that a deleted
  // profile is still selectable and the user can see what's bound (Contract 04
  // edge case: "A profile is deleted from ~/.aws/config after its account was
  // configured → guide the user to fix it").
  const profileOptions = useMemo<{ value: string; label: string }[]>(() => {
    const names = new Set(profiles.map((p) => p.name));
    const opts = profiles.map((p) => ({
      value: p.name,
      label:
        p.source === "sso"
          ? `${p.name} — ${t("profiles.source.sso")}`
          : `${p.name} — ${t("profiles.source.cli")}`,
    }));
    if (target && !names.has(target.profile_name)) {
      opts.unshift({
        value: target.profile_name,
        label: `${target.profile_name} — ${t("accounts.edit.profile_missing")}`,
      });
    }
    return opts;
  }, [profiles, target, t]);

  return (
    <Modal
      open={target !== null}
      onClose={onClose}
      title={t("accounts.edit.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={submitting}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onSubmit}
            disabled={!canSubmit}
            data-testid="accounts-edit-submit"
          >
            {submitting ? t("accounts.edit.saving") : t("common.save")}
          </Button>
        </>
      }
    >
      {target ? (
        <div className="flex flex-col gap-4">
          <p className="text-small text-saw-grey-600 dark:text-saw-grey-400">
            {t("accounts.edit.explainer")}
          </p>

          <label className="flex flex-col gap-1.5">
            <span className="text-small font-medium text-saw-grey-700 dark:text-saw-grey-300">
              {t("accounts.add.label_field")}
            </span>
            <input
              type="text"
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              data-testid="accounts-edit-label"
              maxLength={64}
              className="block w-full rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
            />
          </label>

          <Select<string>
            label={t("accounts.add.profile_field")}
            value={profile}
            options={profileOptions}
            onChange={setProfile}
            data-testid="accounts-edit-profile"
            description={t("accounts.edit.profile_hint")}
          />

          <Select<Environment>
            label={t("accounts.add.environment_field")}
            value={environment}
            options={envOptions}
            onChange={setEnvironment}
            data-testid="accounts-edit-env"
          />

          {error ? (
            <p
              role="alert"
              className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
              data-testid="accounts-edit-error"
            >
              {error}
            </p>
          ) : null}
        </div>
      ) : null}
    </Modal>
  );
}

// --- Remove modal ------------------------------------------------------

function RemoveAccountModal({
  target,
  revealFullId,
  onClose,
  onRemoved,
}: {
  target: Account | null;
  revealFullId: boolean;
  onClose: () => void;
  onRemoved: (impact: RemovalImpact) => Promise<void>;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [typed, setTyped] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (target === null) {
      setTyped("");
      setError(null);
      setSubmitting(false);
    }
  }, [target]);

  if (target === null) {
    // Render an unmounted Modal so the dialog tree stays stable; `open` is
    // the only flag the parent uses to drive visibility.
    return null;
  }

  const targetAccount = target;
  const expected = targetAccount.label;
  const canSubmit = !submitting && typed.trim() === expected;
  const displayedId = revealFullId
    ? targetAccount.aws_account_id
    : maskAccountId(targetAccount.aws_account_id);

  async function onSubmit() {
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    try {
      const impact = await ipc.accountsRemove(targetAccount.aws_account_id);
      await onRemoved(impact);
    } catch (err) {
      setError(formatError(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal
      open={true}
      onClose={onClose}
      title={t("accounts.remove.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={submitting}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="danger"
            onClick={onSubmit}
            disabled={!canSubmit}
            data-testid="accounts-remove-confirm"
          >
            {submitting ? t("accounts.remove.removing") : t("accounts.remove.confirm")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <p className="text-body text-saw-grey-800 dark:text-saw-beige">
          {t("accounts.remove.explainer").replace("{label}", targetAccount.label)}
        </p>
        <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 rounded-card bg-saw-grey-50 dark:bg-saw-black px-4 py-3 text-small">
          <dt className="text-saw-grey-500">{t("accounts.row.profile")}</dt>
          <dd className="font-mono">{targetAccount.profile_name}</dd>
          <dt className="text-saw-grey-500">{t("accounts.row.account_id")}</dt>
          <dd className="font-mono">{displayedId}</dd>
          <dt className="text-saw-grey-500">
            {t("accounts.remove.impact.scans")}
          </dt>
          <dd>{t("accounts.remove.impact.scans_zero")}</dd>
          <dt className="text-saw-grey-500">
            {t("accounts.remove.impact.findings")}
          </dt>
          <dd>{t("accounts.remove.impact.findings_zero")}</dd>
          <dt className="text-saw-grey-500">
            {t("accounts.remove.impact.tf_work")}
          </dt>
          <dd>{t("accounts.remove.impact.tf_work_zero")}</dd>
        </dl>
        <p className="text-small text-saw-grey-700 dark:text-saw-grey-300">
          {t("accounts.remove.permanence")}
        </p>
        <label className="flex flex-col gap-1.5">
          <span className="text-small font-medium text-saw-grey-700 dark:text-saw-grey-300">
            {t("accounts.remove.type_to_confirm").replace("{label}", expected)}
          </span>
          <input
            type="text"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            data-testid="accounts-remove-typed"
            autoComplete="off"
            className="block w-full rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 font-mono text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
          />
        </label>
        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            data-testid="accounts-remove-error"
          >
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}

function formatTs(ts: string): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString();
}

// PR #66: extract the role name from an IAM role ARN. The ARN shape
// is `arn:aws:iam::<account>:role/<name>`; the name is the part after
// `:role/`. Returns undefined when the ARN doesn't carry the marker
// (e.g. an unexpected SDK shape) so the row falls back to the
// status-only label.
function parseRoleNameFromArn(arn: string | undefined): string | undefined {
  if (!arn) return undefined;
  const marker = ":role/";
  const idx = arn.indexOf(marker);
  if (idx === -1) return undefined;
  const name = arn.slice(idx + marker.length);
  return name.length > 0 ? name : undefined;
}

// --- AWS CLI Profiles section ------------------------------------------

/** Single profile row in the AWS CLI Profiles section. Renders the
 *  profile name + source badge + Test button + inline test state
 *  indicator. */
function ProfileRow({
  profile,
  testState,
  onTest,
}: {
  profile: ProfileInfo;
  testState: ProfileTestState | undefined;
  onTest: () => void;
}) {
  const t = useT();
  return (
    <li
      className="flex flex-wrap items-center gap-3 px-4 py-3"
      data-testid={`cli-profile-row-${profile.name}`}
    >
      <div className="min-w-0 flex-1">
        <p className="truncate font-mono text-body text-saw-grey-900 dark:text-saw-beige">
          {profile.name}
        </p>
        <Badge tone="neutral">{profile.source === "sso" ? "SSO" : "CLI"}</Badge>
      </div>
      <div
        className="flex items-center gap-2 text-small"
        data-testid={`cli-profile-test-state-${profile.name}`}
      >
        {testState?.phase === "running" ? (
          <span className="text-saw-grey-600 dark:text-saw-grey-400">
            {t("accounts.cli_profiles.test_running")}
          </span>
        ) : null}
        {testState?.phase === "ok" ? (
          <Badge tone="success">{t("accounts.cli_profiles.test_ok")}</Badge>
        ) : null}
        {testState?.phase === "fail" ? (
          <Badge tone="danger">
            {t("accounts.cli_profiles.test_fail")}
            <span className="ml-1 font-mono text-xs opacity-80">
              ({testState.code})
            </span>
          </Badge>
        ) : null}
        <Button
          variant="secondary"
          size="sm"
          onClick={onTest}
          disabled={testState?.phase === "running"}
          data-testid={`cli-profile-test-${profile.name}`}
        >
          {t("accounts.cli_profiles.test_cta")}
        </Button>
      </div>
    </li>
  );
}

const OUTPUT_FORMATS = ["json", "yaml", "yaml-stream", "text", "table"] as const;
type OutputFormat = (typeof OUTPUT_FORMATS)[number];

/** New AWS CLI profile modal (PR #66). Collects the four values
 *  `aws configure set` would set, forwards them to the
 *  `auth_create_profile` Rust IPC, and lifts success/error back to
 *  the parent via callbacks. The secret access key is not echoed
 *  back from the IPC; once the modal closes the secret state is
 *  released. */
function AddProfileModal({
  open,
  existingProfileNames,
  onClose,
  onSaved,
  onError,
}: {
  open: boolean;
  existingProfileNames: string[];
  onClose: () => void;
  onSaved: (name: string) => Promise<void>;
  onError: (msg: string) => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [name, setName] = useState("");
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [region, setRegion] = useState("us-east-1");
  const [outputFormat, setOutputFormat] = useState<OutputFormat>("json");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset the form whenever the modal opens. Closing the modal
  // simply unmounts the inputs; explicit clearing on open is
  // belt-and-suspenders for secret hygiene.
  useEffect(() => {
    if (open) {
      setName("");
      setAccessKeyId("");
      setSecretAccessKey("");
      setRegion("us-east-1");
      setOutputFormat("json");
      setError(null);
      setSubmitting(false);
    }
  }, [open]);

  const nameValid = /^[A-Za-z0-9_.\-]{1,128}$/.test(name);
  const duplicate = existingProfileNames.includes(name);
  const credsValid = accessKeyId.trim().length > 0 && secretAccessKey.length > 0;
  const canSubmit = !submitting && nameValid && !duplicate && credsValid;

  async function onSubmit() {
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    try {
      await ipc.authCreateProfile({
        name,
        access_key_id: accessKeyId.trim(),
        secret_access_key: secretAccessKey,
        region: region.trim() || undefined,
        output_format: outputFormat,
      });
      // Release the secret from React state before the success
      // callback re-renders the parent. (The state is also cleared
      // on next `open` change, but earlier is better.)
      setSecretAccessKey("");
      setAccessKeyId("");
      await onSaved(name);
    } catch (err) {
      const msg = formatError(err);
      setError(msg);
      // Errors that surface a stable code that the user can act on
      // should bubble to the top-of-page toast too — surfacing in
      // both places makes the failure unmissable.
      onError(msg);
    } finally {
      setSubmitting(false);
    }
  }

  if (!open) return null;

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t("accounts.cli_profiles.add_modal.title")}
      footer={
        <>
          <Button
            variant="ghost"
            onClick={onClose}
            disabled={submitting}
            data-testid="add-profile-cancel"
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void onSubmit()}
            disabled={!canSubmit}
            data-testid="add-profile-save"
          >
            {submitting
              ? t("accounts.cli_profiles.add_modal.saving")
              : t("accounts.cli_profiles.add_modal.save")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <p className="text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("accounts.cli_profiles.add_modal.subtitle")}
        </p>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("accounts.cli_profiles.add_modal.name")}</span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            data-testid="add-profile-name"
            className="rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 font-mono text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-red focus:ring-offset-1"
          />
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("accounts.cli_profiles.add_modal.name_hint")}
          </span>
          {name.length > 0 && !nameValid ? (
            <span role="alert" className="text-xs text-saw-red" data-testid="add-profile-name-error">
              {t("accounts.cli_profiles.add_error_name_invalid")}
            </span>
          ) : null}
          {nameValid && duplicate ? (
            <span role="alert" className="text-xs text-saw-red" data-testid="add-profile-duplicate-error">
              {t("accounts.cli_profiles.add_error_duplicate").replace("{name}", name)}
            </span>
          ) : null}
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("accounts.cli_profiles.add_modal.access_key_id")}</span>
          <input
            type="text"
            value={accessKeyId}
            onChange={(e) => setAccessKeyId(e.target.value)}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            data-testid="add-profile-access-key-id"
            className="rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 font-mono text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-red focus:ring-offset-1"
          />
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("accounts.cli_profiles.add_modal.secret_key")}</span>
          <input
            type="password"
            value={secretAccessKey}
            onChange={(e) => setSecretAccessKey(e.target.value)}
            autoComplete="new-password"
            data-testid="add-profile-secret-key"
            className="rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 font-mono text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-red focus:ring-offset-1"
          />
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("accounts.cli_profiles.add_modal.region")}</span>
          <input
            type="text"
            value={region}
            onChange={(e) => setRegion(e.target.value)}
            placeholder={t("accounts.cli_profiles.add_modal.region_placeholder")}
            autoComplete="off"
            data-testid="add-profile-region"
            className="rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 font-mono text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-red focus:ring-offset-1"
          />
        </label>

        <Select<OutputFormat>
          label={t("accounts.cli_profiles.add_modal.output_format")}
          value={outputFormat}
          options={OUTPUT_FORMATS.map((f) => ({ value: f, label: f }))}
          onChange={(v) => setOutputFormat(v)}
          data-testid="add-profile-output-format"
        />

        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            data-testid="add-profile-error"
          >
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
