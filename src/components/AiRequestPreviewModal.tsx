// Mandatory request-preview modal — Contract 13 §Constraints + §Edge
// Cases. The UI MUST show the full transmitted text before any call;
// the call proceeds only on explicit user action. Canceling sends
// nothing.
//
// What the modal displays:
//   * Provider + model.
//   * The exact `system_prompt` and `user_message` bytes the
//     transport will send (no last-mile rewriting in the client).
//   * The structured finding digest + business context the user
//     message was built from.
//   * Any "looks identifying" flags so the user sees what would be
//     sent verbatim.
//   * The constant placeholders the user message uses for any
//     resource-shaped string, with a "placeholders stay placeholders"
//     reminder.

import { useState } from "react";

import Button from "./Button";
import Modal from "./Modal";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import type { AiRequestPreview, AiSuggestion } from "@/lib/ipc";

type Props = {
  preview: AiRequestPreview | null;
  /** Called when the user clicks "Send to provider". The handler runs
   * `ipc.aiSendRequest(preview)` and resolves with the suggestion. */
  onSend: (preview: AiRequestPreview) => Promise<AiSuggestion>;
  /** Called when the user closes the modal (Cancel or Esc). The
   * contract requires NOTHING be sent on cancel. */
  onClose: () => void;
};

export default function AiRequestPreviewModal({ preview, onSend, onClose }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  if (!preview) return null;

  async function submit() {
    if (!preview) return;
    setBusy(true);
    setErr(null);
    try {
      await onSend(preview);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={true}
      onClose={onClose}
      title={t("ai.preview.title")}
      footer={
        <>
          <Button
            variant="ghost"
            onClick={onClose}
            disabled={busy}
            data-testid="ai-preview-cancel"
          >
            {t("ai.preview.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={busy}
            data-testid="ai-preview-send"
          >
            {busy ? t("ai.preview.sending") : t("ai.preview.send")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-small text-saw-grey-800">
        <p>{t("ai.preview.subtitle")}</p>

        <div className="grid grid-cols-2 gap-3">
          <div>
            <div className="font-medium text-saw-grey-900">
              {t("ai.preview.provider")}
            </div>
            <div className="font-mono text-saw-grey-700">{preview.provider}</div>
          </div>
          <div>
            <div className="font-medium text-saw-grey-900">
              {t("ai.preview.model")}
            </div>
            <div className="font-mono text-saw-grey-700" data-testid="ai-preview-model">
              {preview.model}
            </div>
          </div>
        </div>

        <div>
          <div className="font-medium text-saw-grey-900">
            {t("ai.preview.digest_label")}
          </div>
          <div className="font-mono text-xs text-saw-grey-700">
            <div>rule_key: {preview.digest.rule_key}</div>
            <div>service: {preview.digest.service}</div>
            <div>resource_category: {preview.digest.resource_category}</div>
            <div>severity: {preview.digest.severity}</div>
            <div>
              checked / flagged: {preview.digest.checked_items} /{" "}
              {preview.digest.flagged_items}
            </div>
          </div>
        </div>

        <div>
          <div className="font-medium text-saw-grey-900">
            {t("ai.preview.context_label")}
          </div>
          <div className="font-mono text-xs text-saw-grey-700">
            <div>industry: {preview.context.industry || "(none)"}</div>
            <div>environment_type: {preview.context.environment_type}</div>
            <div>compliance: {preview.context.compliance.join(", ") || "(none)"}</div>
            <div>risk_tolerance: {preview.context.risk_tolerance}</div>
            <div>team_size: {preview.context.team_size}</div>
          </div>
        </div>

        {(preview.flags.industry_identifying || preview.flags.compliance_identifying) ? (
          <div
            className="rounded-card border border-saw-red/30 bg-saw-red/5 px-3 py-2 text-saw-red"
            data-testid="ai-preview-flags"
          >
            <div className="font-medium">{t("ai.preview.flags_label")}</div>
            {preview.flags.industry_identifying ? (
              <div data-testid="ai-preview-flag-industry">
                {t("ai.preview.flag_industry")}
              </div>
            ) : null}
            {preview.flags.compliance_identifying ? (
              <div data-testid="ai-preview-flag-compliance">
                {t("ai.preview.flag_compliance")}
              </div>
            ) : null}
          </div>
        ) : null}

        <div>
          <div className="font-medium text-saw-grey-900">
            {t("ai.preview.placeholders_label")}
          </div>
          <div className="flex flex-wrap gap-1">
            {preview.placeholders_used.map((p) => (
              <span
                key={p}
                className="rounded-full bg-saw-grey-100 px-2 py-0.5 text-xs font-mono text-saw-grey-800"
              >
                {p}
              </span>
            ))}
          </div>
        </div>

        <div>
          <div className="font-medium text-saw-grey-900">
            {t("ai.preview.system_label")}
          </div>
          <pre
            className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-card border border-saw-grey-200 bg-saw-grey-50 p-2 font-mono text-xs text-saw-grey-800"
            data-testid="ai-preview-system"
          >
            {preview.system_prompt}
          </pre>
        </div>

        <div>
          <div className="font-medium text-saw-grey-900">
            {t("ai.preview.user_label")}
          </div>
          <pre
            className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap rounded-card border border-saw-grey-200 bg-saw-grey-50 p-2 font-mono text-xs text-saw-grey-800"
            data-testid="ai-preview-user"
          >
            {preview.user_message}
          </pre>
        </div>

        {err ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
            data-testid="ai-preview-error"
          >
            {err}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
