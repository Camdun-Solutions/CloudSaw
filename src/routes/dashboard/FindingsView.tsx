// FindingsView — the split list+detail surface for a single scan
// (`/scans/:scanId` equivalent). Contract 09 §Expected Output.

import { Fragment, useCallback, useEffect, useMemo, useState } from "react";

import Modal from "@/components/Modal";
import {
  Button,
  EmptyState,
  SafeMarkdown,
  Select,
  SeverityBadge,
  SubmissionPreviewModal,
  VirtualList,
} from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  SEVERITY_ORDER,
  type AiProvider,
  type AiSettings as AiSettingsT,
  type AiSuggestion,
  type ControlMapping,
  type Finding,
  type FindingDetail,
  type FindingResource,
  type FindingStatus,
  type FindingsFilter,
  type FindingTicket,
  type GithubSettings,
  type IssuePreview,
  type KnowledgeArticle,
  type Severity,
} from "@/lib/ipc";

type Props = {
  scanId: string | null;
  onBack: () => void;
};

type SevFilter = "any" | Severity;
type StatusFilter = "any" | FindingStatus;

const ROW_HEIGHT = 64;
const LIST_HEIGHT = 540;
const OTHER_SERVICE = "__other__";

export default function FindingsView({ scanId, onBack }: Props) {
  const t = useT();
  const formatError = useIpcError();

  const [findings, setFindings] = useState<Finding[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [sev, setSev] = useState<SevFilter>("any");
  const [service, setService] = useState<string>("any");
  const [status, setStatus] = useState<StatusFilter>("any");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!scanId) return;
    setFindings(null);
    setError(null);
    setErrorCode(null);
    try {
      const filter: FindingsFilter = {
        severity: sev === "any" ? [] : [sev],
        status: status === "any" ? null : status,
      };
      const list = await ipc.findingsList(scanId, filter);
      setFindings(list);
      if (list.length > 0) {
        setSelectedId((prev) => prev ?? list[0].finding_id);
      }
    } catch (err) {
      setError(formatError(err));
      setErrorCode(
        typeof err === "object" && err !== null && "code" in err
          ? String((err as { code: unknown }).code)
          : "unknown",
      );
      setFindings([]);
    }
  }, [scanId, sev, status, formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Service options come from the loaded findings. Unknown / unrecognized
  // services land under the "Other" bucket per Contract 09 §Constraints.
  const serviceOptions = useMemo(() => {
    const services = new Map<string, number>();
    (findings ?? []).forEach((f) => {
      const key = f.service && f.service.trim() ? f.service : OTHER_SERVICE;
      services.set(key, (services.get(key) ?? 0) + 1);
    });
    const list = Array.from(services.entries())
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([id, count]) => ({
        value: id,
        label:
          id === OTHER_SERVICE
            ? `${t("dashboard.findings.other.service")} (${count})`
            : `${id} (${count})`,
      }));
    return [
      { value: "any", label: t("dashboard.findings.filter.all") },
      ...list,
    ];
  }, [findings, t]);

  const visible = useMemo(() => {
    if (!findings) return [] as Finding[];
    return findings.filter((f) => {
      if (service !== "any") {
        const svc = f.service && f.service.trim() ? f.service : OTHER_SERVICE;
        if (svc !== service) return false;
      }
      return true;
    });
  }, [findings, service]);

  const totalCount = findings?.length ?? 0;
  const matchCount = visible.length;

  if (!scanId) {
    return (
      <EmptyState
        title={t("dashboard.findings.empty.title")}
        body={t("dashboard.findings.empty.body")}
        action={
          <Button onClick={onBack} variant="secondary">
            {t("dashboard.findings.back")}
          </Button>
        }
      />
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={onBack}
          data-testid="findings-back"
        >
          {t("dashboard.findings.back")}
        </Button>
        <h2 className="text-h2 font-semibold">
          {t("dashboard.findings.title").replace(
            "{scan}",
            scanId.slice(0, 8),
          )}
        </h2>
      </div>

      {error ? (
        <ErrorRow message={error} code={errorCode ?? "unknown"} onRetry={reload} />
      ) : null}

      <FilterBar
        sev={sev}
        setSev={setSev}
        service={service}
        setService={setService}
        status={status}
        setStatus={setStatus}
        serviceOptions={serviceOptions}
        onClear={() => {
          setSev("any");
          setService("any");
          setStatus("any");
        }}
      />

      <p
        className="text-small text-saw-grey-600 dark:text-saw-grey-400"
        data-testid="findings-match-count"
      >
        {t("dashboard.findings.filter.match_count")
          .replace("{count}", String(matchCount))
          .replace("{total}", String(totalCount))}
      </p>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
        <FindingsList
          findings={visible}
          selectedId={selectedId}
          onSelect={setSelectedId}
          loading={findings === null}
          hasAny={totalCount > 0}
        />
        <FindingDetailPanel findingId={selectedId} />
      </div>
    </div>
  );
}

// ----- Filter bar ---------------------------------------------------------

type FilterBarProps = {
  sev: SevFilter;
  setSev: (v: SevFilter) => void;
  service: string;
  setService: (v: string) => void;
  status: StatusFilter;
  setStatus: (v: StatusFilter) => void;
  serviceOptions: { value: string; label: string }[];
  onClear: () => void;
};

function FilterBar({
  sev,
  setSev,
  service,
  setService,
  status,
  setStatus,
  serviceOptions,
  onClear,
}: FilterBarProps) {
  const t = useT();
  return (
    <div className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-4">
      <div className="grid grid-cols-1 gap-3 md:grid-cols-4">
        <Select<SevFilter>
          label={t("dashboard.findings.filter.severity")}
          value={sev}
          onChange={setSev}
          options={[
            { value: "any", label: t("dashboard.findings.filter.all") },
            ...SEVERITY_ORDER.map((s) => ({
              value: s as SevFilter,
              label: t(`dashboard.severity.${s}`),
            })),
          ]}
          data-testid="filter-severity"
        />
        <Select<string>
          label={t("dashboard.findings.filter.service")}
          value={service}
          onChange={setService}
          options={serviceOptions}
          data-testid="filter-service"
        />
        <Select<StatusFilter>
          label={t("dashboard.findings.filter.status")}
          value={status}
          onChange={setStatus}
          options={[
            { value: "any", label: t("dashboard.findings.filter.all") },
            { value: "open", label: t("dashboard.status.open") },
            { value: "resolved", label: t("dashboard.status.resolved") },
          ]}
          data-testid="filter-status"
        />
        <div className="flex items-end">
          <Button
            variant="ghost"
            size="sm"
            onClick={onClear}
            data-testid="filter-clear"
          >
            {t("dashboard.findings.filter.clear")}
          </Button>
        </div>
      </div>
    </div>
  );
}

// ----- Findings list (virtualized) ----------------------------------------

type ListProps = {
  findings: Finding[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  loading: boolean;
  hasAny: boolean;
};

function FindingsList({
  findings,
  selectedId,
  onSelect,
  loading,
  hasAny,
}: ListProps) {
  const t = useT();
  if (loading) {
    return (
      <div className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-4 py-8 text-center text-body text-saw-grey-600 dark:text-saw-grey-400">
        {t("common.loading")}
      </div>
    );
  }
  if (findings.length === 0) {
    if (!hasAny) {
      return (
        <EmptyState
          title={t("dashboard.findings.empty.title")}
          body={t("dashboard.findings.empty.body")}
        />
      );
    }
    return (
      <EmptyState
        title={t("dashboard.findings.empty.filtered.title")}
        body={t("dashboard.findings.empty.filtered.body")}
      />
    );
  }
  return (
    <div
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark"
      data-testid="findings-list"
    >
      <VirtualList<Finding>
        items={findings}
        rowHeight={ROW_HEIGHT}
        height={LIST_HEIGHT}
        ariaLabel={t("dashboard.findings.column.title")}
        renderRow={(f) => (
          <FindingRow
            finding={f}
            selected={f.finding_id === selectedId}
            onSelect={() => onSelect(f.finding_id)}
          />
        )}
      />
    </div>
  );
}

function FindingRow({
  finding,
  selected,
  onSelect,
}: {
  finding: Finding;
  selected: boolean;
  onSelect: () => void;
}) {
  const t = useT();
  const title = finding.dashboard_name || finding.rule_key;
  const service =
    finding.service && finding.service.trim()
      ? finding.service
      : t("dashboard.findings.other.service");
  return (
    <button
      type="button"
      onClick={onSelect}
      role="row"
      aria-selected={selected}
      data-testid={`finding-row-${finding.finding_id}`}
      className={[
        "w-full text-left flex items-center gap-3 px-4 py-3 border-b border-saw-grey-100 dark:border-saw-grey-800 last:border-b-0",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange",
        selected ? "bg-saw-grey-100 dark:bg-saw-grey-800" : "bg-saw-white dark:bg-saw-grey-dark hover:bg-saw-grey-50 dark:hover:bg-saw-grey-800",
      ].join(" ")}
      style={{ height: ROW_HEIGHT }}
    >
      <SeverityBadge severity={finding.severity} size="sm" />
      <span className="flex-1 min-w-0">
        <span className="block truncate text-body font-medium text-saw-grey-900 dark:text-saw-beige">
          {title}
        </span>
        <span className="block truncate text-small text-saw-grey-600 dark:text-saw-grey-400">
          {service} · {finding.flagged_items}/{finding.checked_items}
        </span>
      </span>
      <span className="text-small text-saw-grey-700 dark:text-saw-grey-300 whitespace-nowrap">
        {finding.status === "open"
          ? t("dashboard.status.open")
          : t("dashboard.status.resolved")}
      </span>
    </button>
  );
}

// ----- Detail panel -------------------------------------------------------

// Exported so the PR #51 top-level Findings.tsx page can render
// the same panel inline inside each expanded finding row without
// duplicating the KB / AI / GitHub / resource / mapping logic.
// Internal-only call sites (the original FindingsView's right pane)
// still bind the unexported name.
export function FindingDetailPanel({ findingId }: { findingId: string | null }) {
  const t = useT();
  const formatError = useIpcError();
  const [detail, setDetail] = useState<FindingDetail | null>(null);
  const [article, setArticle] = useState<KnowledgeArticle | null>(null);
  const [mapping, setMapping] = useState<ControlMapping | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [ticket, setTicket] = useState<FindingTicket | null>(null);
  const [github, setGithub] = useState<GithubSettings | null>(null);
  const [preview, setPreview] = useState<IssuePreview | null>(null);
  const [aiSettings, setAiSettings] = useState<AiSettingsT | null>(null);
  const [aiSuggestion, setAiSuggestion] = useState<AiSuggestion | null>(null);
  const [aiError, setAiError] = useState<string | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiSetupOpen, setAiSetupOpen] = useState(false);

  const load = useCallback(async () => {
    if (!findingId) return;
    setDetail(null);
    setArticle(null);
    setMapping(null);
    setError(null);
    setErrorCode(null);
    setTicket(null);
    setAiSuggestion(null);
    setAiError(null);
    try {
      const d = await ipc.findingsGet(findingId);
      setDetail(d);
      // Knowledge base + mappings are looked up by `rule_key`, not the
      // SHA-256 finding_id. The KB module returns a default article and
      // empty mappings rather than erroring when there's no match —
      // those don't throw, so we surface them as their respective empty
      // states below.
      const ruleKey = d.finding.rule_key;
      const [art, map, t2, gh, ai] = await Promise.all([
        ipc.kbGetArticle(ruleKey),
        ipc.kbGetControlMappings(ruleKey),
        ipc.githubGetFindingTicket(findingId),
        ipc.githubGetSettings(),
        ipc.aiGetSettings(),
      ]);
      setArticle(art);
      setMapping(map);
      setTicket(t2);
      setGithub(gh);
      setAiSettings(ai);
    } catch (err) {
      setError(formatError(err));
      setErrorCode(
        typeof err === "object" && err !== null && "code" in err
          ? String((err as { code: unknown }).code)
          : "unknown",
      );
    }
  }, [findingId, formatError]);

  useEffect(() => {
    void load();
  }, [load]);

  // PR #84 — One-click AI flow. The previous PR-#58 flow required the
  // user to inspect a verbose preview (system prompt, user message,
  // identifying flags, placeholders…) and explicitly click Send. The
  // user spec for this iteration:
  //
  //   "When a user clicks 'AI suggestion…' the only thing that should
  //    appear is a 'Generating…' loading message and then the actual
  //    recommendation should appear in the text box."
  //
  // So `requestAiSuggestion` now does prepare + send in one shot and
  // surfaces only the "Generating…" state followed by the result.
  // Audit-of-transmitted-bytes (Contract 13 §Constraints) is to be
  // re-homed on the Settings page in a follow-up; the system prompt
  // is static, and the user-message template is deterministic, so a
  // single sample-render in Settings replaces the per-request preview
  // without losing transparency.
  //
  // If the user hasn't connected a provider yet, the button opens the
  // setup modal instead of erroring — that gets them from "I want AI"
  // to "AI works" without bouncing to Settings.
  async function requestAiSuggestion() {
    if (!findingId) return;
    if (!aiSettings || !aiSettings.key_connected) {
      setAiSetupOpen(true);
      return;
    }
    setAiError(null);
    setAiSuggestion(null);
    setAiBusy(true);
    try {
      const preview = await ipc.aiPrepareRequest(findingId);
      const suggestion = await ipc.aiSendRequest(preview);
      setAiSuggestion(suggestion);
    } catch (err) {
      setAiError(formatError(err));
    } finally {
      setAiBusy(false);
    }
  }

  // Called by the setup modal once a provider lands in storage. We
  // refresh aiSettings so the suggestion block flips out of its
  // "not connected" state and immediately kicks off the original
  // request the user was trying to make.
  async function handleAiSetupSaved() {
    setAiSetupOpen(false);
    try {
      const next = await ipc.aiGetSettings();
      setAiSettings(next);
      // Auto-run the suggestion the user was trying to fetch when
      // the setup modal opened. The flow now lands the user on the
      // suggestion result instead of asking them to click again.
      if (next.key_connected && findingId) {
        setAiError(null);
        setAiSuggestion(null);
        setAiBusy(true);
        try {
          const preview = await ipc.aiPrepareRequest(findingId);
          const suggestion = await ipc.aiSendRequest(preview);
          setAiSuggestion(suggestion);
        } catch (err) {
          setAiError(formatError(err));
        } finally {
          setAiBusy(false);
        }
      }
    } catch (err) {
      setAiError(formatError(err));
    }
  }

  // PR #81 — `startCreateTicket` was the in-panel "Create GitHub
  // ticket" handler. The create affordance moved to the Findings
  // drawer header (`FindingGitHubAction`); the legacy in-panel
  // SubmissionPreviewModal mount below stays for one final case the
  // header doesn't cover: the user explicitly clicks "View on
  // GitHub" on the linked-ticket row and wants to re-open the
  // submission preview from the panel itself (rare; defense-in-
  // depth). Modal stays mounted but never opens unless `preview`
  // gets set by something — which today is nothing.

  if (!findingId) {
    return (
      <div
        className="rounded-card border border-dashed border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-6 py-12 text-center text-body text-saw-grey-600 dark:text-saw-grey-400"
        data-testid="finding-detail-empty"
      >
        {t("dashboard.findings.detail.no_selection")}
      </div>
    );
  }

  if (error) {
    return (
      <ErrorRow message={error} code={errorCode ?? "unknown"} onRetry={load} />
    );
  }

  if (!detail || !article || !mapping) {
    return (
      <div className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-4 py-8 text-center text-body text-saw-grey-600 dark:text-saw-grey-400">
        {t("common.loading")}
      </div>
    );
  }

  return (
    <div
      className="space-y-4"
      role="region"
      aria-label={t("dashboard.findings.detail.heading")}
      data-testid="finding-detail-panel"
    >
      <div className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-5">
        <div className="flex items-start gap-3">
          <SeverityBadge severity={detail.finding.severity} />
          <div className="min-w-0">
            {/* PR #82 — prefer the finding's own dashboard_name (human-
                readable, e.g. "Users without MFA") over the article's
                title (often just the rule_key after the matched-flag
                semantics changed). Falls through to article.title then
                rule_key. */}
            <h3 className="text-h2 font-semibold text-saw-grey-900 dark:text-saw-beige">
              {detail.finding.dashboard_name ||
                article.title ||
                detail.finding.rule_key}
            </h3>
            <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
              {detail.finding.rule_key}
            </p>
          </div>
        </div>

        {/* PR #81 — `FindingTicketRow` and its in-panel "Create
            GitHub ticket" button + "No repo selected" hint are gone.
            The create affordance now lives in the Findings drawer
            header via `<FindingGitHubAction>`; the legacy
            "Tracked in {repo}#{n}" link below stays so a linked
            ticket is still visible inside the panel when the user
            has scrolled the drawer body past the header. */}
        {ticket ? (
          <div
            className="mt-3 rounded-card bg-saw-grey-50 dark:bg-saw-black border border-saw-grey-200 dark:border-saw-grey-700 px-3 py-2 text-small flex items-center justify-between"
            data-testid="finding-ticket-linked"
          >
            <span className="text-saw-grey-900 dark:text-saw-beige font-mono">
              {t("findingticket.linked")
                .replace("{repo}", `${ticket.repo.owner}/${ticket.repo.name}`)
                .replace("{n}", String(ticket.issue_number))}
            </span>
            <a
              href={ticket.issue_url}
              target="_blank"
              rel="noopener noreferrer"
              className="text-small text-saw-grey-700 dark:text-saw-grey-300 underline underline-offset-2"
              data-testid="finding-ticket-link"
            >
              {t("findingticket.linked_view")}
            </a>
          </div>
        ) : null}

        {/* PR #82 — always render <ArticleBody>. The backend overlay
            (knowledgebase::scoutsuite::overlay_into_article) now
            guarantees that every article has a non-empty remediation
            sourced from: hand-authored KB → ScoutSuite upstream →
            service-keyed best-practices baseline. The old
            `article.matched ? Body : NoArticleBlock` branch surfaced
            "No remediation guidance yet" even when the overlay had
            populated content — that empty state is gone now. */}
        <ArticleBody article={article} />

        <AiSuggestionBlock
          settings={aiSettings}
          suggestion={aiSuggestion}
          aiError={aiError}
          busy={aiBusy}
          onRequest={() => void requestAiSuggestion()}
        />
      </div>
      <AiQuickSetupModal
        open={aiSetupOpen}
        onClose={() => setAiSetupOpen(false)}
        onSaved={() => void handleAiSetupSaved()}
      />

      <ResourceList detail={detail} />
      <MappingList
        mapping={mapping}
        service={detail.finding.service}
        businessCompliance={aiSettings?.context.compliance ?? null}
      />

      <SubmissionPreviewModal
        preview={preview}
        onClose={() => setPreview(null)}
        onSubmitApi={async (p) => {
          if (!findingId) throw new Error("no finding id");
          const created = await ipc.githubSubmitFindingTicket(findingId, p);
          // Reload so the linked-ticket row replaces the CTA.
          await load();
          return {
            repo: created.repo,
            issue_number: created.issue_number,
            issue_url: created.issue_url,
          };
        }}
        onBrowserFallback={(p) => ipc.githubBrowserFallbackForFinding(p)}
        tokenConfigured={github?.token.configured ?? false}
      />
    </div>
  );
}

// PR #84 — AI suggestion sub-panel, simplified.
//
// Three states only:
//   1. Not configured (no provider key) → "Connect AI Provider" CTA
//      opens the quick-setup modal in the parent.
//   2. Idle / Generating → primary "AI suggestion" CTA, or a
//      "Generating…" indicator while a request is in flight.
//   3. Result → the model's markdown response in a text box, with
//      a small disclaimer + provider/model line below.
//
// The previous inline request preview (system prompt, user message,
// digest, identifying flags, placeholders) is gone from this surface
// per the 2026-05-29 user spec: "The only thing that should appear is
// a 'Generating…' loading message and then the actual recommendation
// should appear in the text box." The static system prompt + the
// deterministic user-message template are scheduled to be re-homed
// on the Settings page so the auditability the inline preview used
// to provide stays available in one place rather than gating every
// suggestion.
function AiSuggestionBlock({
  settings,
  suggestion,
  aiError,
  busy,
  onRequest,
}: {
  settings: AiSettingsT | null;
  suggestion: AiSuggestion | null;
  aiError: string | null;
  busy: boolean;
  onRequest: () => void;
}) {
  const t = useT();
  const ready = !!settings?.key_connected;
  const ctaLabel = ready
    ? t("ai.suggestion.cta")
    : t("ai.suggestion.cta_setup");

  return (
    <div
      className="mt-5 rounded-card border border-dashed border-saw-grey-300 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black p-4"
      data-testid="ai-suggestion-block"
    >
      <div className="flex items-center justify-between">
        <div>
          <span className="rounded-full bg-saw-grey-200 dark:bg-saw-grey-700 px-2 py-0.5 text-xs font-medium text-saw-grey-800 dark:text-saw-beige">
            {t("ai.suggestion.label")}
          </span>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={onRequest}
          disabled={busy}
          data-testid="ai-suggestion-cta"
        >
          {ctaLabel}
        </Button>
      </div>

      {busy ? (
        <p
          className="mt-3 flex items-center gap-2 text-small text-saw-grey-700 dark:text-saw-beige"
          data-testid="ai-suggestion-generating"
          aria-live="polite"
        >
          <span
            className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-saw-grey-300 border-t-saw-orange"
            aria-hidden="true"
          />
          {t("ai.suggestion.generating")}
        </p>
      ) : null}

      {aiError ? (
        <p
          role="alert"
          className="mt-3 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
          data-testid="ai-suggestion-error"
        >
          {aiError}
        </p>
      ) : null}

      {suggestion ? (
        <div className="mt-3" data-testid="ai-suggestion-result">
          <div className="rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-3">
            {suggestion.suggestion_markdown.trim().length > 0 ? (
              <SafeMarkdown markdown={suggestion.suggestion_markdown} />
            ) : (
              <p className="text-small text-saw-grey-600 dark:text-saw-beige">
                {t("ai.suggestion.empty")}
              </p>
            )}
          </div>
          <p className="mt-2 text-xs text-saw-grey-600 dark:text-saw-beige">
            {t("ai.suggestion.disclaimer")}
          </p>
          <div className="mt-1 text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("ai.suggestion.model")}: {suggestion.provider} · {suggestion.model}
            {suggestion.usage_input_tokens !== null &&
            suggestion.usage_output_tokens !== null ? (
              <>
                {" "}
                ·{" "}
                {t("ai.suggestion.usage")
                  .replace("{input}", String(suggestion.usage_input_tokens))
                  .replace("{output}", String(suggestion.usage_output_tokens))}
              </>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}

// PR #84 — Lightweight setup modal that lets a user connect a
// provider without leaving the finding view. Same three fields as
// the Settings → AI "Add Provider" modal (provider type / nickname /
// API key) and routes through the same `aiAddProvider` IPC, so the
// Settings page sees the row immediately. On success the parent
// re-reads aiSettings and auto-fires the suggestion the user was
// trying to fetch when they hit the un-configured state.
function AiQuickSetupModal({
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
    const trimmedNick = nickname.trim();
    if (!trimmedNick) {
      setErr(t("ai.providers.error.no_nickname"));
      return;
    }
    if (!keyInput.trim()) {
      setErr(t("ai.providers.error.no_key"));
      return;
    }
    setBusy(true);
    try {
      await ipc.aiAddProvider(providerType, trimmedNick, keyInput);
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
    <Modal open onClose={onClose} title={t("ai.setup.modal_title")}>
      <div className="flex flex-col gap-4" data-testid="ai-quick-setup-modal">
        <p className="text-small text-saw-grey-700 dark:text-saw-beige">
          {t("ai.setup.modal_subtitle")}
        </p>
        <Select<AiProvider>
          label={t("ai.provider.label")}
          value={providerType}
          options={[
            { value: "anthropic", label: t("ai.provider.anthropic") },
            { value: "openai", label: t("ai.provider.openai") },
            { value: "gemini", label: t("ai.provider.gemini") },
          ]}
          onChange={setProviderType}
          data-testid="ai-quick-setup-type"
        />
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-beige">
          <span>{t("ai.providers.nickname")}</span>
          <input
            type="text"
            value={nickname}
            onChange={(e) => setNickname(e.target.value.slice(0, 60))}
            placeholder={t("ai.providers.nickname_placeholder")}
            maxLength={60}
            className="rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-2 text-body text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1"
            data-testid="ai-quick-setup-nickname"
          />
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("ai.providers.nickname_hint")}
          </span>
        </label>
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-beige">
          <span>{t("ai.key.label")}</span>
          <input
            type="password"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder={t(`ai.key.placeholder_${providerType}`)}
            autoComplete="off"
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige font-mono"
            data-testid="ai-quick-setup-key"
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
            disabled={busy}
            data-testid="ai-quick-setup-cancel"
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={busy}
            data-testid="ai-quick-setup-save"
          >
            {busy ? t("ai.setup.saving") : t("ai.setup.save")}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

// PR #84 — `AiPreviewInline` (the verbose per-request audit panel
// the user had to acknowledge before every Send) is gone. The
// one-click flow in `AiSuggestionBlock` above replaces it; the
// transmitted-bytes audit surface is scheduled to move onto the
// Settings page (single sample render, not per-request) in a
// follow-up.

// PR #81 — `FindingTicketRow` is gone. The create button moved to the
// Findings drawer header (`<FindingGitHubAction>` rendered into
// `Drawer.headerAction`); the in-panel linked-ticket display is now
// inlined at the panel body's render site.

function ArticleBody({ article }: { article: KnowledgeArticle }) {
  const t = useT();

  // PR #83 — Description and Risk render up top, always open if
  // present. Detection Logic is collapsed by default (technical
  // detail). Remediation is now its own flat section — no tabs (the
  // tab UX hid the always-relevant content under a click). Terraform
  // and AWS CLI fixes render as their own sections when the article
  // carries them. False positives lands at the end so the eye flows:
  // what is it → why it matters → how to fix → caveats.
  const sections: { key: string; title: string; body: string; openByDefault: boolean }[] = [
    {
      key: "description",
      title: t("dashboard.findings.detail.section.description"),
      body: article.description,
      openByDefault: true,
    },
    {
      key: "risk",
      title: t("dashboard.findings.detail.section.risk"),
      body: article.risk,
      openByDefault: true,
    },
    {
      key: "remediation",
      title: t("dashboard.findings.detail.section.remediation"),
      body: article.remediation,
      openByDefault: true,
    },
    {
      key: "terraform_fix",
      title: t("dashboard.findings.detail.section.terraform_fix"),
      body: article.terraform_fix,
      openByDefault: false,
    },
    {
      key: "aws_cli_fix",
      title: t("dashboard.findings.detail.section.aws_cli_fix"),
      body: article.aws_cli_fix,
      openByDefault: false,
    },
    {
      key: "detection_logic",
      title: t("dashboard.findings.detail.section.detection_logic"),
      body: article.detection_logic,
      openByDefault: false,
    },
    {
      key: "false_positives",
      title: t("dashboard.findings.detail.section.false_positives"),
      body: article.false_positives,
      openByDefault: false,
    },
  ].filter((s) => s.body && s.body.trim().length > 0);

  // Unmatched H2 sections from forward-compat content land at the end
  // so they aren't silently dropped (matches Contract 08 §Expected
  // Output). PR #83 — the overlay renames the legacy
  // "scoutsuite_references" key to "References" and rewrites bare
  // URLs as `[label](url)` markdown links so SafeMarkdown renders
  // them as proper anchors.
  const extras = Object.entries(article.unmatched_sections ?? {});

  return (
    <div className="mt-3 space-y-2">
      {sections.map((s) => (
        <details
          key={s.key}
          open={s.openByDefault}
          className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-4 py-2"
          data-testid={`kb-section-${s.key}`}
        >
          <summary className="cursor-pointer text-body font-medium text-saw-grey-900 dark:text-saw-beige">
            {s.title}
          </summary>
          <SafeMarkdown
            markdown={s.body}
            className="mt-2"
            data-testid={`kb-section-${s.key}-body`}
          />
        </details>
      ))}

      {extras.map(([h, body]) => (
        <details
          key={`extra-${h}`}
          open
          className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-4 py-2"
        >
          <summary className="cursor-pointer text-body font-medium text-saw-grey-900 dark:text-saw-beige">
            {h}
          </summary>
          <SafeMarkdown markdown={body} className="mt-2" />
        </details>
      ))}
    </div>
  );
}

// PR #82 — `NoArticleBlock` is gone. The backend overlay
// (knowledgebase::scoutsuite::overlay_into_article) now guarantees
// every article has a populated remediation, and the conditional at
// the panel's render site always falls into <ArticleBody>. The
// "No knowledge-base article" empty state is no longer reachable.

function ResourceList({ detail }: { detail: FindingDetail }) {
  const t = useT();
  return (
    <details
      open
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-5"
    >
      <summary className="cursor-pointer text-body font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("dashboard.findings.detail.resources_title")} (
        {detail.resources.length})
      </summary>
      {detail.resources.length === 0 ? (
        <p className="mt-2 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("dashboard.findings.detail.resources.empty")}
        </p>
      ) : (
        <ul className="mt-2 space-y-3">
          {detail.resources.map((r) => (
            <ResourceCard key={r.resource_path} resource={r} />
          ))}
        </ul>
      )}
    </details>
  );
}

/** PR #82 — One row per resource. Renders the human-readable name +
 *  ARN + id when ScoutSuite captured them, plus every captured scalar
 *  attribute (CreateDate, AccessKeys count, etc.) so the user sees
 *  everything ScoutSuite knows about the resource by default. Falls
 *  back to just the dotted path on legacy rows. */
function ResourceCard({ resource }: { resource: FindingResource }) {
  const t = useT();
  const hasIdentity =
    resource.resource_name ||
    resource.resource_arn ||
    resource.resource_id_value;
  const attrEntries = Object.entries(resource.attributes ?? {});
  return (
    <li
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-4 py-3"
      data-testid="resource-card"
    >
      {/* Headline row: human name + invalid badge. Falls back to the
          raw path when ScoutSuite didn't expose an identity at all
          (e.g. `iam.password_policy.*` globals). */}
      <div className="flex items-start justify-between gap-2">
        <span
          className="text-small font-medium text-saw-grey-900 dark:text-saw-beige break-all"
          data-testid="resource-name"
        >
          {resource.resource_name ??
            resource.resource_id_value ??
            resource.resource_path}
        </span>
        {resource.invalid ? (
          <span
            className="shrink-0 rounded-full bg-saw-orange/10 px-2 py-0.5 text-xs text-saw-grey-900 dark:text-saw-beige"
            data-testid="resource-invalid"
          >
            {t("dashboard.findings.detail.resources.invalid")}
          </span>
        ) : null}
      </div>

      {/* ARN + id row */}
      {(resource.resource_arn || resource.resource_id_value) && (
        <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-xs">
          {resource.resource_arn ? (
            <>
              <dt className="font-medium text-saw-grey-500 dark:text-saw-grey-400">
                ARN
              </dt>
              <dd
                className="font-mono text-saw-grey-800 dark:text-saw-beige break-all"
                data-testid="resource-arn"
              >
                {resource.resource_arn}
              </dd>
            </>
          ) : null}
          {resource.resource_id_value ? (
            <>
              <dt className="font-medium text-saw-grey-500 dark:text-saw-grey-400">
                ID
              </dt>
              <dd
                className="font-mono text-saw-grey-800 dark:text-saw-beige break-all"
                data-testid="resource-id"
              >
                {resource.resource_id_value}
              </dd>
            </>
          ) : null}
        </dl>
      )}

      {/* Attribute bag — every other scalar field ScoutSuite emitted.
          Renders alphabetized so the order is stable across renders. */}
      {attrEntries.length > 0 ? (
        <dl
          className="mt-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-xs"
          data-testid="resource-attributes"
        >
          {attrEntries
            .slice()
            .sort(([a], [b]) => a.localeCompare(b))
            .map(([k, v]) => (
              <Fragment key={k}>
                <dt className="font-medium text-saw-grey-500 dark:text-saw-grey-400">
                  {k}
                </dt>
                <dd
                  className="font-mono text-saw-grey-800 dark:text-saw-beige break-all"
                  data-testid={`resource-attr-${k}`}
                >
                  {String(v)}
                </dd>
              </Fragment>
            ))}
        </dl>
      ) : null}

      {/* Raw path — always visible at the end for traceability back to
          the ScoutSuite output, especially when ARN/id aren't shown. */}
      {hasIdentity && (
        <p
          className="mt-2 text-xs text-saw-grey-500 dark:text-saw-grey-400 font-mono break-all"
          data-testid="resource-path"
        >
          {resource.resource_path}
        </p>
      )}
      {!hasIdentity && (
        // No identity → the name slot already showed the path. Don't
        // duplicate it here.
        <></>
      )}
    </li>
  );
}

// PR #81 — coarse service → security-domain mapping. Used as a
// fallback in the Mappings block when a finding has no compliance
// framework entries (and the backend's ScoutSuite synthesis didn't
// land either) so every finding has *some* contextual framing. The
// list is not exhaustive — unknown services fall to "General".
const SERVICE_DOMAIN: Record<string, string> = {
  iam: "Identity & Access Management",
  organizations: "Identity & Access Management",
  sso: "Identity & Access Management",
  cognito: "Identity & Access Management",
  s3: "Data Protection",
  rds: "Data Protection",
  dynamodb: "Data Protection",
  redshift: "Data Protection",
  backup: "Data Protection",
  kms: "Cryptography & Key Management",
  secretsmanager: "Cryptography & Key Management",
  acm: "Cryptography & Key Management",
  ec2: "Network Security",
  vpc: "Network Security",
  elasticloadbalancing: "Network Security",
  elb: "Network Security",
  elbv2: "Network Security",
  cloudfront: "Network Security",
  route53: "Network Security",
  waf: "Network Security",
  shield: "Network Security",
  apigateway: "Network Security",
  cloudtrail: "Logging & Monitoring",
  cloudwatch: "Logging & Monitoring",
  config: "Logging & Monitoring",
  guardduty: "Detection & Response",
  securityhub: "Detection & Response",
  inspector: "Detection & Response",
  macie: "Detection & Response",
  lambda: "Compute Security",
  ecs: "Compute Security",
  ecr: "Compute Security",
  eks: "Compute Security",
  emr: "Data Processing",
  elasticache: "Compute & Caching",
  sns: "Application Integration",
  sqs: "Application Integration",
  ses: "Application Integration",
  cloudformation: "Infrastructure as Code",
};

function securityDomainFor(service: string | undefined): string {
  if (!service) return "General";
  return SERVICE_DOMAIN[service] ?? "General";
}

// PR #83 — Map BusinessContext.compliance pill labels (free-form,
// user-facing — "SOC2", "ISO27001", "PCI-DSS", "NIST-800-53", …) to
// the bundled framework IDs we store mappings under
// ("soc2"/"iso27001"/"pcidss"/"nist"/…). Both sides of the lookup
// are normalized: lower-cased + non-alphanumerics stripped, so
// `NIST-800-53` and `nist 800 53` and `nist80053` all collide on
// the same key. Aliases live here too — `NIST-CSF` and `NIST-800-53`
// both resolve to our single bundled `nist` framework because we
// don't ship separate mapping sets for them.
const FRAMEWORK_ALIASES: Record<string, string> = {
  soc2: "soc2",
  soc: "soc2",
  iso27001: "iso27001",
  iso2700122022: "iso27001",
  hipaa: "hipaa",
  nist: "nist",
  nist80053: "nist",
  nistcsf: "nist",
  pcidss: "pcidss",
  pci: "pcidss",
  cis: "cis",
};

function normalizeFrameworkName(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function resolveBundledFrameworkId(pillLabel: string): string | null {
  return FRAMEWORK_ALIASES[normalizeFrameworkName(pillLabel)] ?? null;
}

function MappingList({
  mapping,
  service,
  businessCompliance,
}: {
  mapping: ControlMapping;
  service?: string;
  /** PR #83 — Compliance pill list from BusinessContext. The
   *  Mappings panel filters to display only CIS (always, as a basic
   *  baseline) + frameworks matching this list. Null = settings
   *  haven't loaded yet → render nothing instead of all-or-nothing
   *  to avoid a flash of the wrong shape. Empty list = user hasn't
   *  scoped any frameworks → CIS only. */
  businessCompliance: string[] | null;
}) {
  const t = useT();
  const allEntries = Object.entries(mapping.frameworks ?? {});
  const domain = securityDomainFor(service);

  // Resolve user-scoped pill labels to bundled framework ids. CIS is
  // always included as the "basic" baseline per spec.
  const allowedIds = useMemo(() => {
    const out = new Set<string>(["cis"]);
    for (const pill of businessCompliance ?? []) {
      const resolved = resolveBundledFrameworkId(pill);
      if (resolved) out.add(resolved);
    }
    return out;
  }, [businessCompliance]);

  const entries =
    businessCompliance === null
      ? []
      : allEntries.filter(([fwId]) => allowedIds.has(fwId));

  return (
    <details
      open
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-5"
      data-testid="mappings-block"
    >
      <summary className="cursor-pointer text-body font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("dashboard.findings.detail.mappings.title")}
      </summary>
      {/* PR #81 — the security-domain chip renders ABOVE the framework
          list (or alone when nothing else matches). Every finding gets
          a domain — coarse but always non-empty — so even findings the
          KB hasn't touched have *some* topical framing. */}
      <p
        className="mt-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
        data-testid="mappings-security-domain"
      >
        <strong>{t("dashboard.findings.detail.mappings.security_domain")}:</strong>{" "}
        {domain}
      </p>
      {entries.length === 0 ? (
        <div
          className="mt-2 rounded-card border border-dashed border-saw-grey-300 dark:border-saw-grey-700 px-4 py-3 text-small text-saw-grey-600 dark:text-saw-grey-400"
          data-testid="mappings-empty"
        >
          <strong>
            {t("dashboard.findings.detail.mappings.empty.title")}
          </strong>
          <p>{t("dashboard.findings.detail.mappings.empty.body")}</p>
        </div>
      ) : (
        <div className="mt-2 space-y-3">
          {entries.map(([fw, controls]) => (
            <div key={fw}>
              <h4 className="text-small font-semibold uppercase tracking-wide text-saw-grey-600 dark:text-saw-grey-400">
                {fw}
              </h4>
              <ul className="mt-1 space-y-1">
                {controls.map((c) => (
                  <li
                    key={`${fw}-${c.control_id}`}
                    className="text-small text-saw-grey-800 dark:text-saw-beige"
                    data-testid={`mapping-${fw}-${c.control_id}`}
                  >
                    <span className="font-mono">{c.control_id}</span>{" "}
                    — {c.title}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </details>
  );
}

// ----- Error helpers ------------------------------------------------------

function ErrorRow({
  message,
  code,
  onRetry,
}: {
  message: string;
  code: string;
  onRetry: () => void;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const diagnostic = `code=${code} message=${message}`;
  return (
    <div
      role="alert"
      className="rounded-card border border-saw-red/40 bg-saw-red/5 px-4 py-3"
      data-testid="findings-error"
    >
      <p className="text-body text-saw-grey-900 dark:text-saw-beige">
        {t("dashboard.findings.detail.error").replace("{code}", code)}
      </p>
      <p className="mt-1 text-small text-saw-grey-700 dark:text-saw-grey-300">{message}</p>
      <div className="mt-2 flex items-center gap-3">
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard?.writeText(diagnostic).then(
              () => {
                setCopied(true);
                window.setTimeout(() => setCopied(false), 2000);
              },
              () => {},
            );
          }}
          className="text-small text-saw-grey-700 dark:text-saw-grey-300 underline underline-offset-2"
          data-testid="findings-error-copy"
        >
          {copied
            ? t("dashboard.findings.detail.copy_diagnostic.copied")
            : t("dashboard.findings.detail.copy_diagnostic")}
        </button>
        <button
          type="button"
          onClick={onRetry}
          className="text-small text-saw-grey-700 dark:text-saw-grey-300 underline underline-offset-2"
        >
          {t("common.confirm")}
        </button>
      </div>
    </div>
  );
}
