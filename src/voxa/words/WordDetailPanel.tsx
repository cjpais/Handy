import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { formatNumber } from "../stats/format";
import { barRatio } from "./format";
import type { DetailBlock } from "./blocks";

interface Props {
  word: string;
  detail: DetailBlock | null;
  locale: string;
  onClose: () => void;
}

/**
 * Side panel for one word: every form with its count, and the latest dictations
 * that use it with the word highlighted. Esc or the backdrop closes it.
 */
export const WordDetailPanel: React.FC<Props> = ({
  word,
  detail,
  locale,
  onClose,
}) => {
  const { t } = useTranslation();
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const data = detail?.status === "ready" ? detail.data : null;
  const maxForm = data ? Math.max(0, ...data.forms.map((f) => f.count)) : 0;

  return (
    <>
      <div className="vgw-scrim" onClick={onClose} aria-hidden="true" />
      <aside
        className="vgw-panel vg-card"
        role="dialog"
        aria-modal="false"
        aria-label={t("voxa.words.detail.title")}
        data-testid="word-detail"
      >
        <header className="vgw-panel-head">
          <div>
            <p className="vg-eyebrow">{t("voxa.words.detail.title")}</p>
            <h3 className="vgw-panel-word">{word}</h3>
            {data && (
              <p className="vg-muted">
                {t("voxa.words.detail.total", {
                  count: formatNumber(data.total, locale),
                })}
              </p>
            )}
          </div>
          <button
            ref={closeRef}
            type="button"
            className="vg-round-btn"
            aria-label={t("voxa.words.detail.close")}
            title={t("voxa.words.detail.close")}
            onClick={onClose}
          >
            <X aria-hidden="true" />
          </button>
        </header>

        {detail?.status === "error" && (
          <p className="vg-muted">
            {t("voxa.words.error", { error: detail.error })}
          </p>
        )}
        {(!detail || detail.status === "loading") && (
          <p className="vg-muted" aria-busy="true">
            {t("voxa.words.loading")}
          </p>
        )}

        {data && (
          <>
            <section>
              <h4 className="vg-eyebrow">{t("voxa.words.detail.forms")}</h4>
              <ul className="vgw-forms">
                {data.forms.map((f) => (
                  <li key={f.form}>
                    <span className="vgw-form-name">{f.form}</span>
                    <span className="vgw-form-bar" aria-hidden="true">
                      <i
                        style={{
                          width: `${barRatio(f.count, maxForm) * 100}%`,
                        }}
                      />
                    </span>
                    <span className="vg-tabular">
                      {formatNumber(f.count, locale)}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
            <section>
              <h4 className="vg-eyebrow">{t("voxa.words.detail.examples")}</h4>
              {data.examples.length === 0 ? (
                <p className="vg-muted">{t("voxa.words.detail.noExamples")}</p>
              ) : (
                <ul className="vgw-examples">
                  {data.examples.map((ex) => (
                    <li key={ex.dictation_id}>
                      <time
                        className="vg-muted vgw-example-time"
                        dateTime={new Date(ex.created_at * 1000).toISOString()}
                      >
                        {new Intl.DateTimeFormat(locale, {
                          dateStyle: "medium",
                          timeStyle: "short",
                        }).format(new Date(ex.created_at * 1000))}
                      </time>
                      <p>
                        {ex.segments.map((s, i) =>
                          s.hit ? (
                            <mark key={i}>{s.text}</mark>
                          ) : (
                            <React.Fragment key={i}>{s.text}</React.Fragment>
                          ),
                        )}
                      </p>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </>
        )}
      </aside>
    </>
  );
};
