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

import { Badge, Button, EmptyState, Logo, Modal, Select } from "@/components";
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
  type RemovalImpact,
  type UpdateAccountInput,
} from "@/lib/ipc";
import ConnectScannerRoleForm from "@/components/ConnectScannerRoleForm";
import ScanProgressModal from "@/routes/ScanProgress";

type Props = {
  /** Standalone-route close handler. Required unless `embedded` is
   *  true (embedded mode is owned by the host Settings page and
   *  has no separate close affordance). */
  onClose?: () => void;
  onOpenProfiles: () => void;
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
  onClose,
  onOpenProfiles,
  embedded = false,
}: Props) {
  const t = useT();
  const formatError = useIpcError();

  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  // Names of profiles currently in `~/.aws/config`. Used to flag accounts
  // whose profile was deleted out-of-band (Contract 04 §Edge Cases).
  const [presentProfiles, setPresentProfiles] = useState<Set<string>>(new Set());
  const [display, setDisplay] = useState<AccountsDisplaySettings>({
    reveal_full_ids: false,
  });
  const [loadError, setLoadError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  // Modal state. The "edit" modal accepts the row to edit; the "remove" modal
  // accepts the row to remove. Add modal is the simpler boolean form. The
  // provisioning modal (Contract 05) is per-row, like edit/remove.
  const [addOpen, setAddOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<Account | null>(null);
  const [removeTarget, setRemoveTarget] = useState<Account | null>(null);
  const [provisionTarget, setProvisionTarget] = useState<Account | null>(null);
  const [scanTarget, setScanTarget] = useState<Account | null>(null);

  const reload = useCallback(async () => {
    setRefreshing(true);
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
    } catch (err) {
      setLoadError(formatError(err));
      setAccounts([]);
    } finally {
      setRefreshing(false);
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

  // In embedded mode (rendered inside Settings, PR #46), the page-
  // level header is hidden and the "Refresh" / "Open profiles"
  // actions reflow into a compact action row directly above the
  // configured-accounts list. In standalone mode (legacy route),
  // the original full-page header renders unchanged.
  const standaloneHeader = (
    <header className="border-b border-saw-grey-200 bg-saw-white px-8 py-5">
      <div className="flex items-center gap-3">
        <Logo size="sm" />
        <div className="flex flex-col">
          <h1 className="text-h2 font-semibold tracking-tight">
            {t("accounts.title")}
          </h1>
          <p className="text-small text-saw-grey-500">
            {t("accounts.subtitle")}
          </p>
        </div>
        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={onOpenProfiles}
            data-testid="accounts-open-profiles"
          >
            {t("accounts.open_profiles")}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void reload()}
            disabled={refreshing}
            data-testid="accounts-refresh"
          >
            {refreshing ? t("accounts.refreshing") : t("accounts.refresh")}
          </Button>
          {onClose ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={onClose}
              data-testid="accounts-close"
            >
              {t("common.close")}
            </Button>
          ) : null}
        </div>
      </div>
    </header>
  );

  // Embedded-mode action row — same Open Profiles + Refresh
  // affordances that lived in the standalone header, just laid
  // out as a compact row above the section content.
  const embeddedActions = (
    <div className="mb-4 flex items-center justify-end gap-2">
      <Button
        variant="ghost"
        size="sm"
        onClick={onOpenProfiles}
        data-testid="accounts-open-profiles"
      >
        {t("accounts.open_profiles")}
      </Button>
      <Button
        variant="secondary"
        size="sm"
        onClick={() => void reload()}
        disabled={refreshing}
        data-testid="accounts-refresh"
      >
        {refreshing ? t("accounts.refreshing") : t("accounts.refresh")}
      </Button>
    </div>
  );

  // Body content — identical in both modes. Section padding/max-
  // width is dropped in embedded mode because the host Settings
  // page already imposes its own layout container.
  const body = (
    <>
      {embedded ? embeddedActions : null}
      <section
        className={
          embedded
            ? "flex flex-col gap-3"
            : "mx-auto max-w-4xl px-8 py-10"
        }
      >
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-h3 font-semibold tracking-tight">
              {t("accounts.section.configured")}
            </h2>
            <p className="mt-1 max-w-2xl text-small text-saw-grey-600">
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

        <label className="mt-4 inline-flex items-center gap-2 text-small text-saw-grey-600">
          <input
            type="checkbox"
            checked={display.reveal_full_ids}
            onChange={(e) => void onToggleReveal(e.target.checked)}
            data-testid="accounts-reveal-toggle"
            className="h-4 w-4 rounded border-saw-grey-300"
          />
          {t("accounts.reveal_toggle.label")}
        </label>

        {loadError ? (
          <p
            role="alert"
            className="mt-6 rounded-card border border-saw-grey-200 bg-saw-white px-4 py-3 text-body text-saw-red"
            data-testid="accounts-load-error"
          >
            {loadError}
          </p>
        ) : null}

        <div className="mt-6" data-testid="accounts-list">
          {accounts === null ? (
            <p className="text-body text-saw-grey-600">{t("common.loading")}</p>
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
              className="divide-y divide-saw-grey-200 rounded-card border border-saw-grey-200 bg-saw-white"
              data-testid="accounts-rows"
            >
              {accounts.map((a) => (
                <AccountRow
                  key={a.aws_account_id}
                  account={a}
                  active={a.aws_account_id === activeId}
                  revealFullId={display.reveal_full_ids}
                  profileMissing={!presentProfiles.has(a.profile_name)}
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

      {/* Phase 2 — "Connect scanner role" modal. Wraps the shared
        * ConnectScannerRoleForm component so the per-account action on
        * this page surfaces the same flow as onboarding step 4. */}
      {provisionTarget ? (
        <Modal
          open={true}
          onClose={() => setProvisionTarget(null)}
          title={"Connect scanner role"}
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
    <main className="min-h-full bg-saw-grey-50 text-saw-grey-900">
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

  return (
    <li className="px-5 py-4" data-testid={`account-row-${account.aws_account_id}`}>
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <p
              className="truncate text-body font-medium"
              data-testid={`account-label-${account.aws_account_id}`}
            >
              {account.label}
            </p>
            <Badge tone="neutral" data-testid={`account-env-${account.aws_account_id}`}>
              {t(`accounts.env.${account.environment}`)}
            </Badge>
            {active ? (
              <Badge
                tone="info"
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
              className="mt-2 rounded-card bg-saw-grey-50 px-3 py-2 text-small text-saw-grey-700"
              data-testid={`account-invalid-hint-${account.aws_account_id}`}
            >
              {t("accounts.row.profile_missing_hint").replace(
                "{profile}",
                account.profile_name,
              )}
            </p>
          ) : null}
          <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-0.5 text-small text-saw-grey-600">
            <dt className="text-saw-grey-500">{t("accounts.row.profile")}</dt>
            <dd className="font-mono">{account.profile_name}</dd>
            <dt className="text-saw-grey-500">{t("accounts.row.account_id")}</dt>
            <dd
              className="font-mono"
              data-testid={`account-id-${account.aws_account_id}`}
            >
              {displayedId}
            </dd>
            <dt className="text-saw-grey-500">{t("accounts.row.role_status")}</dt>
            <dd>
              {account.role_provisioned
                ? t("accounts.row.role_provisioned")
                : t("accounts.row.role_not_provisioned")}
            </dd>
            <dt className="text-saw-grey-500">{t("accounts.row.last_scan")}</dt>
            <dd>
              {account.last_scan_at
                ? formatTs(account.last_scan_at)
                : t("accounts.row.never_scanned")}
            </dd>
          </dl>
        </div>
        <div className="flex flex-col items-end gap-2">
          {active ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={onSetActive}
              data-testid={`account-active-cta-${account.aws_account_id}`}
              disabled
            >
              {t("accounts.row.is_active")}
            </Button>
          ) : (
            <Button
              variant="secondary"
              size="sm"
              onClick={onSetActive}
              data-testid={`account-set-active-${account.aws_account_id}`}
            >
              {t("accounts.row.set_active")}
            </Button>
          )}
          <Button
            variant={account.role_provisioned ? "ghost" : "primary"}
            size="sm"
            onClick={onProvision}
            data-testid={`account-provision-${account.aws_account_id}`}
          >
            {account.role_provisioned
              ? t("terraform.provision.replan_cta")
              : t("terraform.provision.cta")}
          </Button>
          {account.role_provisioned ? (
            <Button
              variant="primary"
              size="sm"
              onClick={onScan}
              data-testid={`account-scan-${account.aws_account_id}`}
            >
              {t("scanner.scan.cta")}
            </Button>
          ) : null}
          <div className="flex gap-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={onEdit}
              data-testid={`account-edit-${account.aws_account_id}`}
            >
              {t("accounts.row.edit")}
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={onRemove}
              data-testid={`account-remove-${account.aws_account_id}`}
            >
              {t("accounts.row.remove")}
            </Button>
          </div>
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
        <p className="text-small text-saw-grey-600">
          {t("accounts.add.explainer")}
        </p>

        <label className="flex flex-col gap-1.5">
          <span className="text-small font-medium text-saw-grey-700">
            {t("accounts.add.label_field")}
          </span>
          <input
            type="text"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            data-testid="accounts-add-label"
            maxLength={64}
            className="block w-full rounded-card border border-saw-grey-300 bg-saw-white px-3 py-2 text-body text-saw-grey-900 focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
          />
          <span className="text-small text-saw-grey-500">
            {t("accounts.add.label_hint")}
          </span>
        </label>

        {profilesLoaded && profiles.length === 0 ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-50 px-3 py-2 text-small text-saw-grey-700"
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
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
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
          <p className="text-small text-saw-grey-600">
            {t("accounts.edit.explainer")}
          </p>

          <label className="flex flex-col gap-1.5">
            <span className="text-small font-medium text-saw-grey-700">
              {t("accounts.add.label_field")}
            </span>
            <input
              type="text"
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              data-testid="accounts-edit-label"
              maxLength={64}
              className="block w-full rounded-card border border-saw-grey-300 bg-saw-white px-3 py-2 text-body text-saw-grey-900 focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
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
              className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
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
        <p className="text-body text-saw-grey-800">
          {t("accounts.remove.explainer").replace("{label}", targetAccount.label)}
        </p>
        <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 rounded-card bg-saw-grey-50 px-4 py-3 text-small">
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
        <p className="text-small text-saw-grey-700">
          {t("accounts.remove.permanence")}
        </p>
        <label className="flex flex-col gap-1.5">
          <span className="text-small font-medium text-saw-grey-700">
            {t("accounts.remove.type_to_confirm").replace("{label}", expected)}
          </span>
          <input
            type="text"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            data-testid="accounts-remove-typed"
            autoComplete="off"
            className="block w-full rounded-card border border-saw-grey-300 bg-saw-white px-3 py-2 font-mono text-body text-saw-grey-900 focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
          />
        </label>
        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
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
