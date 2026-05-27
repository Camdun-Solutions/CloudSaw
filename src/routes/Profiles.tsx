// Profiles — discovered AWS profiles and a per-profile "Test" button that
// runs `sts:GetCallerIdentity` (Contract 03).
//
// This is a diagnostics surface. CloudSaw-local *Accounts* (Contract 04) are
// the user-facing concept; profiles are the underlying AWS CLI configuration
// that accounts reference. The Accounts page uses the same `auth_test_profile`
// IPC under the hood during the add-account flow, but exposes the raw
// profile list here for troubleshooting (SSO expired, permission denied, …)
// without forcing users back through the add wizard.
//
// The screen never sees credential material — the IPC contract returns
// only profile names, identity strings, and enumerated status values.

import { useCallback, useEffect, useState, type ReactNode } from "react";

import { BackBreadcrumb, Badge, Button, EmptyState, Logo } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type ProfileInfo,
  type ProfileTestResult,
  type TestFailureReason,
} from "@/lib/ipc";

type Props = { onClose: () => void };

const FAILURE_KEY: Record<TestFailureReason, string> = {
  profile_not_configured: "aws.failure.profile_not_configured",
  sso_expired: "aws.failure.sso_expired",
  permission_denied: "aws.failure.permission_denied",
  connectivity: "aws.failure.connectivity",
  timeout: "aws.failure.timeout",
  other: "aws.failure.other",
};

export default function Profiles({ onClose }: Props) {
  const t = useT();
  const formatError = useIpcError();

  const [profiles, setProfiles] = useState<ProfileInfo[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  // Per-profile test state, keyed by profile name. Separate maps so
  // pending/result/error don't fight over a single slot.
  const [testing, setTesting] = useState<Record<string, boolean>>({});
  const [results, setResults] = useState<Record<string, ProfileTestResult>>({});

  const load = useCallback(async () => {
    setRefreshing(true);
    setLoadError(null);
    try {
      const list = await ipc.authListProfiles();
      setProfiles(list);
    } catch (err) {
      setLoadError(formatError(err));
      setProfiles([]);
    } finally {
      setRefreshing(false);
    }
  }, [formatError]);

  useEffect(() => {
    void load();
  }, [load]);

  async function onTest(name: string) {
    setTesting((m) => ({ ...m, [name]: true }));
    try {
      const result = await ipc.authTestProfile(name);
      setResults((m) => ({ ...m, [name]: result }));
    } catch (err) {
      // `auth_test_profile` only throws on a *programmer* error (invalid
      // profile name). Everything else lands as a `failure` shape. Surface
      // the thrown error via the localized formatter and clear any prior
      // success so the row reflects the latest attempt.
      setResults((m) => ({
        ...m,
        [name]: {
          status: "failure",
          reason: "other",
          api: null,
        },
      }));
      // Stash the formatted message so the row's failure card can show it
      // — but only as a supplementary line; the primary message comes from
      // the enumerated reason.
      console.error(formatError(err));
    } finally {
      setTesting((m) => ({ ...m, [name]: false }));
    }
  }

  return (
    <main className="min-h-full bg-saw-grey-50 text-saw-grey-900">
      <header className="border-b border-saw-grey-200 bg-saw-white px-8 py-5">
        {/* PR #49: back-arrow breadcrumb replaces the old Close
            text button. Profiles is a sub-page of Settings (the
            embedded Accounts panel's "Open profiles" button is
            the only entry point post PR #46), so Back-to-Settings. */}
        <div className="mb-2">
          <BackBreadcrumb
            destination={t("nav.settings")}
            onBack={onClose}
            data-testid="profiles-back"
          />
        </div>
        <div className="flex items-center gap-3">
          <Logo size="sm" />
          <div className="flex flex-col">
            <h1 className="text-h2 font-semibold tracking-tight">
              {t("profiles.title")}
            </h1>
            <p className="text-small text-saw-grey-500">{t("app.tagline")}</p>
          </div>
          <div className="ml-auto flex items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void load()}
              disabled={refreshing}
              data-testid="profiles-refresh"
            >
              {refreshing ? t("profiles.refreshing") : t("profiles.refresh")}
            </Button>
          </div>
        </div>
      </header>

      <section className="mx-auto max-w-4xl px-8 py-10">
        <p className="max-w-2xl text-body text-saw-grey-600">
          {t("profiles.subtitle")}
        </p>

        {loadError ? (
          <p
            role="alert"
            className="mt-6 rounded-card border border-saw-grey-200 bg-saw-white px-4 py-3 text-body text-saw-red"
            data-testid="profiles-load-error"
          >
            {loadError}
          </p>
        ) : null}

        <div className="mt-8" data-testid="profiles-list">
          {profiles === null ? (
            <p className="text-body text-saw-grey-600">{t("common.loading")}</p>
          ) : profiles.length === 0 ? (
            <EmptyState
              title={t("profiles.empty.title")}
              body={t("profiles.empty.body")}
              action={
                <Button
                  variant="primary"
                  onClick={() => void load()}
                  disabled={refreshing}
                >
                  {refreshing
                    ? t("profiles.refreshing")
                    : t("profiles.refresh")}
                </Button>
              }
            />
          ) : (
            <ul className="divide-y divide-saw-grey-200 rounded-card border border-saw-grey-200 bg-saw-white">
              {profiles.map((p) => (
                <ProfileRow
                  key={p.name}
                  profile={p}
                  testing={!!testing[p.name]}
                  result={results[p.name]}
                  onTest={() => onTest(p.name)}
                />
              ))}
            </ul>
          )}
        </div>
      </section>
    </main>
  );
}

function ProfileRow({
  profile,
  testing,
  result,
  onTest,
}: {
  profile: ProfileInfo;
  testing: boolean;
  result?: ProfileTestResult;
  onTest: () => void;
}) {
  const t = useT();

  let statusBadge: ReactNode;
  if (testing) {
    statusBadge = <Badge tone="info">{t("profiles.testing")}</Badge>;
  } else if (!result) {
    statusBadge = (
      <Badge tone="neutral" data-testid={`profile-status-${profile.name}`}>
        {t("profiles.status.untested")}
      </Badge>
    );
  } else if (result.status === "success") {
    statusBadge = (
      <Badge tone="success" data-testid={`profile-status-${profile.name}`}>
        {t("profiles.status.success")}
      </Badge>
    );
  } else {
    statusBadge = (
      <Badge tone="danger" data-testid={`profile-status-${profile.name}`}>
        {t("profiles.status.failed")}
      </Badge>
    );
  }

  return (
    <li className="px-5 py-4">
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <p
            className="truncate text-body font-medium"
            data-testid={`profile-name-${profile.name}`}
          >
            {profile.name}
          </p>
          <p className="text-small text-saw-grey-500">
            {profile.source === "sso"
              ? t("profiles.source.sso")
              : t("profiles.source.cli")}
          </p>
        </div>
        <div className="flex items-center gap-3">
          {statusBadge}
          <Button
            variant="secondary"
            size="sm"
            onClick={onTest}
            disabled={testing}
            data-testid={`profile-test-${profile.name}`}
          >
            {testing ? t("profiles.testing") : t("profiles.test")}
          </Button>
        </div>
      </div>

      {result?.status === "success" ? (
        <dl
          className="mt-3 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 rounded-card bg-saw-grey-50 px-4 py-3 text-small"
          data-testid={`profile-success-${profile.name}`}
        >
          <dt className="text-saw-grey-500">{t("profiles.result.account")}</dt>
          <dd className="font-mono text-saw-grey-900">
            {result.identity.account_id}
          </dd>
          <dt className="text-saw-grey-500">{t("profiles.result.arn")}</dt>
          <dd className="break-all font-mono text-saw-grey-900">
            {result.identity.arn}
          </dd>
          <dt className="text-saw-grey-500">{t("profiles.result.user_id")}</dt>
          <dd className="break-all font-mono text-saw-grey-900">
            {result.identity.user_id}
          </dd>
        </dl>
      ) : null}

      {result?.status === "failure" ? (
        <div
          role="alert"
          className="mt-3 rounded-card bg-saw-grey-50 px-4 py-3 text-small"
          data-testid={`profile-failure-${profile.name}`}
        >
          <p className="text-saw-grey-900">{t(FAILURE_KEY[result.reason])}</p>
          {result.api ? (
            <p className="mt-1 text-saw-grey-500">
              {t("profiles.result.failed_api")}: <span className="font-mono">{result.api}</span>
            </p>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}
