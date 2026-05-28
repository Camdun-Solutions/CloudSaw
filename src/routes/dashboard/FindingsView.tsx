// FindingsView — the split list+detail surface for a single scan
// (`/scans/:scanId` equivalent). Contract 09 §Expected Output.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  AiRequestPreviewModal,
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
  type AiRequestPreview,
  type AiSettings as AiSettingsT,
  type AiSuggestion,
  type ControlMapping,
  type Finding,
  type FindingDetail,
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
  const [aiPreview, setAiPreview] = useState<AiRequestPreview | null>(null);
  const [aiSuggestion, setAiSuggestion] = useState<AiSuggestion | null>(null);
  const [aiError, setAiError] = useState<string | null>(null);

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

  async function startAiSuggestion() {
    if (!findingId) return;
    setAiError(null);
    setAiSuggestion(null);
    if (!aiSettings || !aiSettings.key_connected) {
      // Contract 13 §Edge Cases: no key → direct the user to Settings.
      setAiError(t("ai.error.no_provider_key"));
      return;
    }
    try {
      const p = await ipc.aiPrepareRequest(findingId);
      setAiPreview(p);
    } catch (err) {
      setAiError(formatError(err));
    }
  }

  async function sendAiRequest(p: AiRequestPreview): Promise<AiSuggestion> {
    const suggestion = await ipc.aiSendRequest(p);
    setAiSuggestion(suggestion);
    setAiPreview(null);
    return suggestion;
  }

  async function startCreateTicket() {
    if (!findingId || !github) return;
    if (!github.findings_repo) {
      setError(t("github.error.no_findings_repo"));
      return;
    }
    if (ticket) {
      // Already linked — the UI shows the link rather than filing a
      // duplicate (Contract 12 §Edge Cases).
      return;
    }
    try {
      const p = await ipc.githubPrepareFindingTicket(findingId, github.findings_repo);
      setPreview(p);
    } catch (err) {
      setError(formatError(err));
    }
  }

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
            <h3 className="text-h2 font-semibold text-saw-grey-900 dark:text-saw-beige">
              {article.matched
                ? article.title
                : detail.finding.dashboard_name || detail.finding.rule_key}
            </h3>
            <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
              {detail.finding.rule_key}{" "}
              {!article.matched ? (
                <span
                  className="ml-2 rounded-full bg-saw-grey-100 dark:bg-saw-grey-800 px-2 py-0.5 text-saw-grey-700 dark:text-saw-grey-300"
                  data-testid="kb-unmatched-tag"
                >
                  {t("dashboard.findings.unmatched_label")}
                </span>
              ) : (
                <span className="ml-2 rounded-full bg-saw-grey-100 dark:bg-saw-grey-800 px-2 py-0.5 text-saw-grey-700 dark:text-saw-grey-300">
                  {article.source === "bundled"
                    ? t("dashboard.findings.detail.section.kb_source.bundled")
                    : t("dashboard.findings.detail.section.kb_source.remote")}
                </span>
              )}
            </p>
          </div>
        </div>

        <FindingTicketRow
          ticket={ticket}
          onCreate={() => void startCreateTicket()}
          tokenConfigured={github?.token.configured ?? false}
          findingsRepoConfigured={!!github?.findings_repo}
        />

        {article.matched ? (
          <ArticleBody article={article} />
        ) : (
          <NoArticleBlock finding={detail.finding} />
        )}

        <AiSuggestionBlock
          settings={aiSettings}
          suggestion={aiSuggestion}
          aiError={aiError}
          onStart={() => void startAiSuggestion()}
        />
      </div>

      <ResourceList detail={detail} />
      <MappingList mapping={mapping} />

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

      <AiRequestPreviewModal
        preview={aiPreview}
        onClose={() => setAiPreview(null)}
        onSend={sendAiRequest}
      />
    </div>
  );
}

// AI suggestion sub-panel — visually distinct from the KB article above.
// Renders an opt-in CTA, the disabled-hint pointing to Settings if no key
// is connected, and (after a Send) the suggestion with a clear
// "AI-generated, unreviewed" label + placeholder reminder + token usage.
function AiSuggestionBlock({
  settings,
  suggestion,
  aiError,
  onStart,
}: {
  settings: AiSettingsT | null;
  suggestion: AiSuggestion | null;
  aiError: string | null;
  onStart: () => void;
}) {
  const t = useT();
  const ready = !!settings?.key_connected;

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
          variant={ready ? "secondary" : "ghost"}
          size="sm"
          onClick={onStart}
          data-testid="ai-suggestion-cta"
        >
          {t("ai.suggestion.cta")}
        </Button>
      </div>
      {!ready ? (
        <p
          className="mt-2 text-small text-saw-grey-600 dark:text-saw-grey-400"
          data-testid="ai-suggestion-disabled-hint"
        >
          {t("ai.suggestion.disabled_hint")}
        </p>
      ) : null}
      {aiError ? (
        <p
          role="alert"
          className="mt-2 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
          data-testid="ai-suggestion-error"
        >
          {aiError}
        </p>
      ) : null}
      {suggestion ? (
        <div className="mt-3" data-testid="ai-suggestion-result">
          <p className="text-xs text-saw-grey-600 dark:text-saw-grey-400">
            {t("ai.suggestion.disclaimer")}
          </p>
          <p className="mt-1 text-xs text-saw-grey-600 dark:text-saw-grey-400">
            {t("ai.suggestion.placeholders_note")}
          </p>
          <div className="mt-3 rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-3">
            {suggestion.suggestion_markdown.trim().length > 0 ? (
              <SafeMarkdown markdown={suggestion.suggestion_markdown} />
            ) : (
              <p className="text-small text-saw-grey-600 dark:text-saw-grey-400">
                {t("ai.suggestion.empty")}
              </p>
            )}
          </div>
          <div className="mt-2 text-xs text-saw-grey-500 dark:text-saw-grey-400">
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

function FindingTicketRow({
  ticket,
  onCreate,
  tokenConfigured,
  findingsRepoConfigured,
}: {
  ticket: FindingTicket | null;
  onCreate: () => void;
  tokenConfigured: boolean;
  findingsRepoConfigured: boolean;
}) {
  const t = useT();
  if (ticket) {
    return (
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
    );
  }
  return (
    <div className="mt-3">
      <Button
        variant="secondary"
        size="sm"
        onClick={onCreate}
        disabled={!findingsRepoConfigured && !tokenConfigured}
        data-testid="finding-create-ticket"
      >
        {t("findingticket.cta")}
      </Button>
      {!findingsRepoConfigured ? (
        <p className="mt-1 text-xs text-saw-grey-500 dark:text-saw-grey-400" data-testid="finding-create-ticket-hint">
          {t("github.findings_repo.none")}
        </p>
      ) : null}
    </div>
  );
}

function ArticleBody({ article }: { article: KnowledgeArticle }) {
  const t = useT();
  const sections: { key: string; title: string; body: string }[] = [
    {
      key: "description",
      title: t("dashboard.findings.detail.section.description"),
      body: article.description,
    },
    {
      key: "risk",
      title: t("dashboard.findings.detail.section.risk"),
      body: article.risk,
    },
    {
      key: "detection_logic",
      title: t("dashboard.findings.detail.section.detection_logic"),
      body: article.detection_logic,
    },
    {
      key: "remediation",
      title: t("dashboard.findings.detail.section.remediation"),
      body: article.remediation,
    },
    {
      key: "terraform_fix",
      title: t("dashboard.findings.detail.section.terraform_fix"),
      body: article.terraform_fix,
    },
    {
      key: "aws_cli_fix",
      title: t("dashboard.findings.detail.section.aws_cli_fix"),
      body: article.aws_cli_fix,
    },
    {
      key: "false_positives",
      title: t("dashboard.findings.detail.section.false_positives"),
      body: article.false_positives,
    },
  ].filter((s) => s.body && s.body.trim().length > 0);

  // Unmatched H2 sections from forward-compat content land at the end so
  // they aren't silently dropped (matches Contract 08 §Expected Output).
  const extras = Object.entries(article.unmatched_sections ?? {});

  return (
    <div className="mt-3 space-y-2">
      {sections.map((s) => (
        <details
          key={s.key}
          open={
            s.key === "description" ||
            s.key === "risk" ||
            s.key === "remediation"
          }
          className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-4 py-2"
        >
          <summary className="cursor-pointer text-body font-medium text-saw-grey-900 dark:text-saw-beige">
            {s.title}
          </summary>
          <SafeMarkdown
            markdown={s.body}
            className="mt-2"
            data-testid={`kb-section-${s.key}`}
          />
        </details>
      ))}
      {extras.map(([h, body]) => (
        <details
          key={`extra-${h}`}
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

function NoArticleBlock({ finding }: { finding: Finding }) {
  const t = useT();
  return (
    <div className="mt-4 rounded-card border border-dashed border-saw-grey-300 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-4 py-4">
      <h4 className="text-body font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("dashboard.findings.detail.no_article.title")}
      </h4>
      <p className="mt-1 text-body text-saw-grey-700 dark:text-saw-grey-300">
        {t("dashboard.findings.detail.no_article.body")}
      </p>
      <div className="mt-3 space-y-2 text-small text-saw-grey-700 dark:text-saw-grey-300">
        <p>
          <strong>{t("dashboard.findings.detail.section.description")}:</strong>{" "}
          {finding.description}
        </p>
        {finding.rationale ? (
          <p>
            <strong>{t("dashboard.findings.detail.section.risk")}:</strong>{" "}
            {finding.rationale}
          </p>
        ) : null}
      </div>
      <p className="mt-3 text-small">
        <a
          href={t("dashboard.findings.contrib.url")}
          target="_blank"
          rel="noopener noreferrer"
          className="text-saw-red underline underline-offset-2"
          data-testid="kb-contribute-link"
        >
          {t("dashboard.findings.detail.no_article.contribute")}
        </a>
      </p>
    </div>
  );
}

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
        <ul className="mt-2 space-y-1">
          {detail.resources.map((r) => (
            <li
              key={r.resource_path}
              className="font-mono text-small text-saw-grey-800 dark:text-saw-beige break-all"
            >
              {r.resource_path}
              {r.invalid ? (
                <span
                  className="ml-2 rounded-full bg-saw-orange/10 px-2 py-0.5 text-saw-grey-900 dark:text-saw-beige"
                  data-testid="resource-invalid"
                >
                  {t("dashboard.findings.detail.resources.invalid")}
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </details>
  );
}

function MappingList({ mapping }: { mapping: ControlMapping }) {
  const t = useT();
  const entries = Object.entries(mapping.frameworks ?? {});
  return (
    <details
      open
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-5"
      data-testid="mappings-block"
    >
      <summary className="cursor-pointer text-body font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("dashboard.findings.detail.mappings.title")}
      </summary>
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
