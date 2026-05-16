import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ExternalLink, Loader2, Plus, X } from "lucide-react";
import "./AgentReviewOverlay.css";

type AgentReviewStatus = "pending" | "approved" | "cancelled" | "failed";

interface AgentReviewRequest {
  id: string;
  title: string;
  actionName: string;
  toolName: string;
  argumentsJson: string;
  status: AgentReviewStatus;
  resultJson?: string | null;
  error?: string | null;
  resolutionJson?: string | null;
}

interface AgentToolOverlay {
  id: string;
  title: string;
  toolName: string;
  resultJson: string;
}

type ReviewFields = Record<string, unknown>;

interface ToolResultItem {
  title: string;
  detail?: string;
  url?: string;
}

interface RelationCandidate {
  title: string;
  url: string;
}

interface RelationSelection {
  propertyName: string;
  recordType?: string;
  query: string;
  message: string;
  candidates: RelationCandidate[];
  canCreate?: boolean;
}

const LEAD_FIELD_ORDER = [
  "company",
  "contactName",
  "role",
  "email",
  "phone",
  "source",
  "status",
  "ownerName",
  "nextStep",
  "notes",
];

const DEAL_FIELD_ORDER = [
  "dealName",
  "company",
  "contactName",
  "amount",
  "stage",
  "closeDate",
  "source",
  "ownerName",
  "nextStep",
  "notes",
];

const TASK_FIELD_ORDER = [
  "taskName",
  "ownerName",
  "dueDate",
  "priority",
  "status",
  "relatedCompany",
  "relatedDeal",
  "relatedEngagement",
  "relatedContact",
  "clientTaskType",
  "team",
  "notes",
];

function tryParseJson(value: unknown) {
  if (typeof value !== "string") return value;

  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}

function fieldsFromReview(review: AgentReviewRequest | null) {
  if (!review) return {};

  try {
    const args = JSON.parse(review.argumentsJson) as ReviewFields;
    const firstPage = Array.isArray(args.pages)
      ? (args.pages[0] as ReviewFields | undefined)
      : undefined;
    const content = tryParseJson(firstPage?.content ?? args.content);
    if (content && typeof content === "object" && !Array.isArray(content)) {
      return content as ReviewFields;
    }
    return firstPage ?? args;
  } catch {
    return {};
  }
}

function formatValue(value: unknown) {
  if (value === null || value === undefined || value === "") return null;
  if (typeof value === "string") return value;
  return JSON.stringify(value);
}

function compactString(value: unknown, fallback = "") {
  return typeof value === "string" && value.trim() ? value.trim() : fallback;
}

function parseJsonString(value: unknown): unknown {
  if (typeof value !== "string") return value;
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}

function textFromMcpContent(value: unknown) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const content = (value as ReviewFields).content;
  if (!Array.isArray(content)) return null;
  return content
    .map((item) =>
      item && typeof item === "object"
        ? compactString((item as ReviewFields).text)
        : "",
    )
    .filter(Boolean)
    .join("\n\n");
}

function firstString(record: ReviewFields, keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return null;
}

function collectUrlItems(value: unknown, items: ToolResultItem[] = []) {
  if (!value || items.length >= 8) return items;

  if (Array.isArray(value)) {
    value.forEach((entry) => collectUrlItems(entry, items));
    return items;
  }

  if (typeof value !== "object") return items;

  const record = value as ReviewFields;
  const url = firstString(record, ["url", "public_url", "link", "href"]);
  if (url) {
    items.push({
      title:
        firstString(record, [
          "title",
          "name",
          "summary",
          "subject",
          "dealName",
          "company",
        ]) ?? url,
      detail:
        firstString(record, ["detail", "stage", "snippet", "description", "text", "body"]) ??
        undefined,
      url,
    });
  }

  Object.values(record).forEach((entry) => collectUrlItems(entry, items));
  return items;
}

function toolItems(toolName: string, parsed: unknown): ToolResultItem[] {
  if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
    const record = parsed as ReviewFields;

    if (Array.isArray(record.events)) {
      return record.events.slice(0, 8).map((event) => {
        const eventRecord = event as ReviewFields;
        return {
          title: compactString(eventRecord.summary, "(No title)"),
          detail: [eventRecord.start, eventRecord.end]
            .map((value) => compactString(value))
            .filter(Boolean)
            .join(" - "),
        };
      });
    }

    if (Array.isArray(record.messages)) {
      return record.messages.slice(0, 8).map((message) => {
        const messageRecord = message as ReviewFields;
        return {
          title: compactString(messageRecord.subject, "(No subject)"),
          detail: compactString(messageRecord.snippet),
        };
      });
    }
  }

  const urlItems = collectUrlItems(parsed);
  if (urlItems.length > 0) return urlItems;

  const mcpText = textFromMcpContent(parsed);
  const text =
    mcpText ??
    (typeof parsed === "string" ? parsed : JSON.stringify(parsed, null, 2));
  const lines = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, toolName === "calendar_check_availability" ? 3 : 8);

  return lines.map((line) => {
    const url = line.match(/https?:\/\/\S+/)?.[0];
    return {
      title: line.replace(/https?:\/\/\S+/, "").trim() || url || line,
      url,
    };
  });
}

function parseToolOverlay(overlay: AgentToolOverlay | null) {
  if (!overlay) return { parsed: null, items: [] as ToolResultItem[] };
  const parsed = parseJsonString(overlay.resultJson);
  const contentText = textFromMcpContent(parsed);
  const contentParsed = parseJsonString(contentText);
  const displayValue = contentParsed ?? parsed;
  return {
    parsed: displayValue,
    items: toolItems(overlay.toolName, displayValue),
  };
}

function relationSelectionFromReview(
  review: AgentReviewRequest | null,
): RelationSelection | null {
  if (!review?.resolutionJson) return null;

  try {
    return JSON.parse(review.resolutionJson) as RelationSelection;
  } catch {
    return null;
  }
}

const AgentReviewOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [review, setReview] = useState<AgentReviewRequest | null>(null);
  const [toolOverlay, setToolOverlay] = useState<AgentToolOverlay | null>(null);
  const [isApproving, setIsApproving] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);
  const [isSelectingRelation, setIsSelectingRelation] = useState(false);
  const [isCreatingRelation, setIsCreatingRelation] = useState(false);
  const [manualRelationUrl, setManualRelationUrl] = useState("");
  const [relationActionError, setRelationActionError] = useState<string | null>(
    null,
  );

  useEffect(() => {
    invoke<AgentReviewRequest | null>("get_agent_review")
      .then(setReview)
      .catch(console.warn);
    invoke<AgentToolOverlay | null>("get_agent_tool_overlay")
      .then(setToolOverlay)
      .catch(console.warn);

    const unlisten = listen<AgentReviewRequest>(
      "agent-review-updated",
      (event) => {
        setReview(event.payload.status === "cancelled" ? null : event.payload);
      },
    );
    const unlistenTool = listen<AgentToolOverlay | null>(
      "agent-tool-overlay-updated",
      (event) => {
        setToolOverlay(event.payload);
      },
    );

    return () => {
      unlisten.then((fn) => fn());
      unlistenTool.then((fn) => fn());
    };
  }, []);

  const fields = useMemo(() => {
    const reviewFields = fieldsFromReview(review);
    const fieldOrder = (() => {
      if (review?.actionName === "notion_deal") return DEAL_FIELD_ORDER;
      if (review?.actionName === "notion_task") return TASK_FIELD_ORDER;
      return LEAD_FIELD_ORDER;
    })();
    const knownEntries = fieldOrder.map(
      (key) => [key, reviewFields[key]] as const,
    );
    const extraEntries = Object.entries(reviewFields).filter(
      ([key]) => !fieldOrder.includes(key),
    );

    return [...knownEntries, ...extraEntries]
      .map(([key, value]) => [key, formatValue(value)] as const)
      .filter((entry): entry is readonly [string, string] => Boolean(entry[1]));
  }, [review]);

  const relationSelection = useMemo(
    () => relationSelectionFromReview(review),
    [review],
  );

  useEffect(() => {
    setManualRelationUrl("");
    setRelationActionError(null);
  }, [review?.resolutionJson]);

  const approve = async () => {
    setIsApproving(true);
    try {
      const nextReview = await invoke<AgentReviewRequest>(
        "approve_agent_review",
      );
      setReview(nextReview.status === "approved" ? null : nextReview);
    } catch (error) {
      console.warn(error);
    } finally {
      setIsApproving(false);
    }
  };

  const cancel = async () => {
    setIsCancelling(true);
    try {
      await invoke("cancel_agent_review");
      setReview(null);
    } catch (error) {
      console.warn(error);
    } finally {
      setIsCancelling(false);
    }
  };

  const selectRelation = async (propertyName: string, url: string) => {
    if (!url.trim()) return;
    setIsSelectingRelation(true);
    setRelationActionError(null);
    try {
      const nextReview = await invoke<AgentReviewRequest>(
        "select_agent_review_relation",
        { propertyName, url: url.trim() },
      );
      setReview(nextReview);
    } catch (error) {
      setRelationActionError(String(error));
      console.warn(error);
    } finally {
      setIsSelectingRelation(false);
    }
  };

  const createRelation = async (propertyName: string) => {
    setIsCreatingRelation(true);
    setRelationActionError(null);
    try {
      const nextReview = await invoke<AgentReviewRequest>(
        "create_agent_review_relation",
        { propertyName },
      );
      setReview(nextReview);
    } catch (error) {
      setRelationActionError(String(error));
      console.warn(error);
    } finally {
      setIsCreatingRelation(false);
    }
  };

  const clearToolOverlay = async () => {
    try {
      await invoke("clear_agent_tool_overlay");
      setToolOverlay(null);
    } catch (error) {
      console.warn(error);
    }
  };

  const { items: resultItems } = useMemo(
    () => parseToolOverlay(toolOverlay),
    [toolOverlay],
  );

  if (!review && !toolOverlay) return null;

  if (!review && toolOverlay) {
    return (
      <section className="agent-review-overlay">
        <div className="agent-review-header">
          <div>
            <p className="agent-review-kicker">
              {t("overlay.agentReview.resultKicker")}
            </p>
            <h1>{toolOverlay.title}</h1>
          </div>
          <button
            className="agent-review-icon-button"
            type="button"
            aria-label={t("overlay.agentReview.dismiss")}
            onClick={() => void clearToolOverlay()}
          >
            <X size={16} />
          </button>
        </div>

        <div className="agent-review-content">
          {resultItems.length > 0 ? (
            <div className="agent-result-list">
              {resultItems.map((item, index) => (
                <a
                  className={`agent-result-item ${item.url ? "" : "agent-result-item-static"}`}
                  href={item.url}
                  target="_blank"
                  rel="noreferrer"
                  key={`${item.title}-${index}`}
                >
                  <span className="agent-result-title">{item.title}</span>
                  {item.detail && (
                    <span className="agent-result-detail">{item.detail}</span>
                  )}
                  {item.url && <ExternalLink size={14} />}
                </a>
              ))}
            </div>
          ) : (
            <p className="agent-review-empty">
              {t("overlay.agentReview.noResults")}
            </p>
          )}
        </div>
      </section>
    );
  }

  const activeReview = review;
  if (!activeReview) return null;

  return (
    <section className="agent-review-overlay">
      <div className="agent-review-header">
        <div>
          <p className="agent-review-kicker">
            {t("overlay.agentReview.kicker")}
          </p>
          <h1>{t("overlay.agentReview.title")}</h1>
        </div>
        <button
          className="agent-review-icon-button"
          type="button"
          aria-label={t("overlay.agentReview.cancel")}
          onClick={() => void cancel()}
          disabled={isApproving || isCancelling}
        >
          <X size={16} />
        </button>
      </div>

      <div className="agent-review-content">
        {relationSelection && (
          <div className="agent-relation-selection agent-relation-selection-top">
            <h3>
              {t("overlay.agentReview.chooseRelation", {
                property: relationSelection.propertyName,
              })}
            </h3>
            <p>{relationSelection.message}</p>
            {relationSelection.candidates.length > 0 ? (
              <div className="agent-relation-options">
                {relationSelection.candidates.map((candidate) => (
                  <button
                    className="agent-relation-option"
                    type="button"
                    key={candidate.url}
                    disabled={isSelectingRelation || isCreatingRelation}
                    onClick={() =>
                      void selectRelation(
                        relationSelection.propertyName,
                        candidate.url,
                      )
                    }
                  >
                    <span>{candidate.title}</span>
                    <ExternalLink size={14} />
                  </button>
                ))}
              </div>
            ) : (
              <p className="agent-review-empty">
                {t("overlay.agentReview.noRelationCandidates")}
              </p>
            )}
            {relationSelection.canCreate && (
              <button
                className="agent-relation-create"
                type="button"
                disabled={isSelectingRelation || isCreatingRelation}
                onClick={() =>
                  void createRelation(relationSelection.propertyName)
                }
              >
                {isCreatingRelation ? (
                  <Loader2 className="agent-review-spin" size={14} />
                ) : (
                  <Plus size={14} />
                )}
                <span>
                  {t("overlay.agentReview.createRelation", {
                    type:
                      relationSelection.recordType ??
                      relationSelection.propertyName,
                    name: relationSelection.query,
                  })}
                </span>
              </button>
            )}
            <div className="agent-relation-manual">
              <input
                type="url"
                value={manualRelationUrl}
                placeholder={t("overlay.agentReview.pasteRelationUrl")}
                onChange={(event) => setManualRelationUrl(event.target.value)}
              />
              <button
                type="button"
                disabled={
                  !manualRelationUrl.trim() ||
                  isSelectingRelation ||
                  isCreatingRelation
                }
                onClick={() =>
                  void selectRelation(
                    relationSelection.propertyName,
                    manualRelationUrl,
                  )
                }
              >
                {t("overlay.agentReview.useRelationUrl")}
              </button>
            </div>
            {relationActionError && (
              <p className="agent-relation-error">{relationActionError}</p>
            )}
          </div>
        )}
        <h2>{activeReview.title}</h2>
        {fields.length > 0 ? (
          <dl className="agent-review-fields">
            {fields.map(([key, value]) => (
              <div className="agent-review-field" key={key}>
                <dt>
                  {t(`overlay.agentReview.fields.${key}`, {
                    defaultValue: key,
                  })}
                </dt>
                <dd>{value}</dd>
              </div>
            ))}
          </dl>
        ) : (
          <p className="agent-review-empty">
            {t("overlay.agentReview.noFields")}
          </p>
        )}
      </div>

      {activeReview.status === "failed" && activeReview.error && (
        <p className="agent-review-error">{activeReview.error}</p>
      )}

      <div className="agent-review-actions">
        <button
          className="agent-review-secondary"
          type="button"
          onClick={() => void cancel()}
          disabled={isApproving || isCancelling}
        >
          {isCancelling ? (
            <Loader2 className="agent-review-spin" size={16} />
          ) : (
            <X size={16} />
          )}
          <span>{t("overlay.agentReview.cancel")}</span>
        </button>
        <button
          className="agent-review-primary"
          type="button"
          onClick={() => void approve()}
          disabled={isApproving || isCancelling || Boolean(relationSelection)}
        >
          {isApproving ? (
            <Loader2 className="agent-review-spin" size={16} />
          ) : (
            <Check size={16} />
          )}
          <span>
            {t(
              activeReview.actionName === "notion_deal"
                ? "overlay.agentReview.approveDeal"
                : activeReview.actionName === "notion_task"
                  ? "overlay.agentReview.approveTask"
                  : "overlay.agentReview.approveLead",
            )}
          </span>
        </button>
      </div>
    </section>
  );
};

export default AgentReviewOverlay;
