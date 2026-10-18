import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  BackfillStatus,
  FillerStat,
  OpenerStat,
  PhraseStat,
  WordCount,
} from "@/bindings";
import { Button } from "@/components/ui/Button";
import { formatNumber, pluralForm } from "../stats/format";
import type { Block, DetailBlock, WordsData } from "./blocks";
import { WordDetailPanel } from "./WordDetailPanel";
import { Sparkline, TrendLine } from "./TrendLine";
import { barRatio, formatDecimal, type PeriodPreset } from "./format";

/** Top words shown before "Show all". */
const COLLAPSED_ROWS = 15;

export interface SelectedWord {
  stem: string;
  word: string;
}

export interface WordsViewProps {
  data: WordsData;
  locale: string;
  preset: PeriodPreset;
  custom: { from: string; to: string };
  onPreset: (preset: PeriodPreset) => void;
  onCustom: (from: string, to: string) => void;
  includeStop: boolean;
  onIncludeStop: (value: boolean) => void;
  backfill: BackfillStatus | null;
  selected: SelectedWord | null;
  detail: DetailBlock | null;
  onSelect: (word: SelectedWord | null) => void;
  onRetry: () => void;
}

// ---------------------------------------------------------------- pieces

/** Renders one block's three states; `children` gets the data when ready. */
function BlockBody<T>({
  block,
  onRetry,
  children,
}: {
  block: Block<T>;
  onRetry: () => void;
  children: (data: T) => React.ReactNode;
}) {
  const { t } = useTranslation();
  if (block.status === "loading") {
    return (
      <p className="vg-muted vg-empty-line" aria-busy="true">
        {t("voxa.words.loading")}
      </p>
    );
  }
  if (block.status === "error") {
    return (
      <div className="vgw-block-error" role="alert">
        <p className="vg-muted">
          {t("voxa.words.error", { error: block.error })}
        </p>
        <Button variant="secondary" size="sm" onClick={onRetry}>
          {t("voxa.words.retry")}
        </Button>
      </div>
    );
  }
  return <>{children(block.data)}</>;
}

const Empty: React.FC = () => {
  const { t } = useTranslation();
  return <p className="vg-muted vg-empty-line">{t("voxa.words.emptyBlock")}</p>;
};

const WordRows: React.FC<{
  words: WordCount[];
  locale: string;
  onSelect: (w: SelectedWord) => void;
  testId: string;
}> = ({ words, locale, onSelect, testId }) => {
  const max = words.reduce((m, w) => Math.max(m, w.count), 0);
  return (
    <ul className="vgw-rows" data-testid={testId}>
      {words.map((w) => (
        <li key={w.stem}>
          <button
            type="button"
            className="vgw-row"
            onClick={() => onSelect({ stem: w.stem, word: w.word })}
          >
            <span className="vgw-row-label">{w.word}</span>
            <span className="vgw-row-bar" aria-hidden="true">
              <i style={{ width: `${barRatio(w.count, max) * 100}%` }} />
            </span>
            <span className="vgw-row-count vg-tabular">
              {formatNumber(w.count, locale)}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
};

/** Plain (non-clickable) rows: openers and phrases. */
const TextRows: React.FC<{
  rows: { text: string; count: number; percent?: number }[];
  locale: string;
  testId: string;
}> = ({ rows, locale, testId }) => {
  const max = rows.reduce((m, r) => Math.max(m, r.count), 0);
  return (
    <ul className="vgw-rows" data-testid={testId}>
      {rows.map((r) => (
        <li key={r.text} className="vgw-row is-static">
          <span className="vgw-row-label">{r.text}</span>
          <span className="vgw-row-bar" aria-hidden="true">
            <i style={{ width: `${barRatio(r.count, max) * 100}%` }} />
          </span>
          <span className="vgw-row-count vg-tabular">
            {r.percent !== undefined
              ? `${formatDecimal(r.percent, locale, r.percent < 10 ? 1 : 0)}%`
              : `${formatNumber(r.count, locale)}×`}
          </span>
        </li>
      ))}
    </ul>
  );
};

const Card: React.FC<{
  title: string;
  hint?: string;
  testId: string;
  className?: string;
  children: React.ReactNode;
}> = ({ title, hint, testId, className = "", children }) => (
  <section className={`vg-card vgw-card ${className}`} data-testid={testId}>
    <h3 className="vg-card-title">{title}</h3>
    {hint && <p className="vg-muted vgw-hint">{hint}</p>}
    {children}
  </section>
);

// ---------------------------------------------------------------- controls

const PERIODS: PeriodPreset[] = ["week", "month", "all", "custom"];

const PeriodFilter: React.FC<{
  preset: PeriodPreset;
  custom: { from: string; to: string };
  onPreset: (p: PeriodPreset) => void;
  onCustom: (from: string, to: string) => void;
}> = ({ preset, custom, onPreset, onCustom }) => {
  const { t } = useTranslation();
  return (
    <div className="vgw-period">
      <div
        className="vg-segmented"
        role="group"
        aria-label={t("voxa.words.period.label")}
      >
        {PERIODS.map((p) => (
          <button
            key={p}
            type="button"
            className="vg-segment"
            aria-pressed={preset === p}
            title={
              p === "week" || p === "month"
                ? t(`voxa.words.period.${p}Hint`)
                : undefined
            }
            onClick={() => onPreset(p)}
          >
            {t(`voxa.words.period.${p}`)}
          </button>
        ))}
      </div>
      {preset === "custom" && (
        <div className="vgw-custom">
          <label>
            <span className="vg-muted">{t("voxa.words.period.from")}</span>
            <input
              type="date"
              value={custom.from}
              max={custom.to || undefined}
              onChange={(e) => onCustom(e.target.value, custom.to)}
            />
          </label>
          <label>
            <span className="vg-muted">{t("voxa.words.period.to")}</span>
            <input
              type="date"
              value={custom.to}
              min={custom.from || undefined}
              onChange={(e) => onCustom(custom.from, e.target.value)}
            />
          </label>
        </div>
      )}
    </div>
  );
};

// ---------------------------------------------------------------- view

export const WordsView: React.FC<WordsViewProps> = ({
  data,
  locale,
  preset,
  custom,
  onPreset,
  onCustom,
  includeStop,
  onIncludeStop,
  backfill,
  selected,
  detail,
  onSelect,
  onRetry,
}) => {
  const { t } = useTranslation();
  const [showAll, setShowAll] = useState(false);

  const vocabulary =
    data.vocabulary.status === "ready" ? data.vocabulary.data : null;
  const noWordsAtAll =
    preset === "all" && vocabulary !== null && vocabulary.total === 0;
  const indexing = backfill?.state === "running";

  return (
    <div className="vg-stats vgw" data-testid="words-view">
      <div className="vgw-controls">
        <PeriodFilter
          preset={preset}
          custom={custom}
          onPreset={onPreset}
          onCustom={onCustom}
        />
        <label className="vgw-switch" title={t("voxa.words.showStop.hint")}>
          <input
            type="checkbox"
            checked={includeStop}
            onChange={(e) => onIncludeStop(e.target.checked)}
          />
          <span>{t("voxa.words.showStop.label")}</span>
        </label>
      </div>

      {indexing && backfill && (
        <div className="vg-card vgw-backfill" role="status">
          <p>
            {t("voxa.words.backfill.running", {
              processed: formatNumber(backfill.processed, locale),
              total: formatNumber(backfill.total, locale),
            })}
          </p>
          <progress
            max={Math.max(backfill.total, 1)}
            value={backfill.processed}
            aria-label={t("voxa.words.backfill.running", {
              processed: backfill.processed,
              total: backfill.total,
            })}
          />
          <p className="vg-muted">{t("voxa.words.backfill.hint")}</p>
        </div>
      )}

      {noWordsAtAll && !indexing ? (
        <div className="vg-card vg-empty" data-testid="words-empty">
          <h3 className="vg-card-title">{t("voxa.words.empty.title")}</h3>
          <p className="vg-muted">{t("voxa.words.empty.body")}</p>
        </div>
      ) : (
        <>
          <Card
            title={t("voxa.words.vocabulary.title")}
            testId="words-vocabulary"
            className="vgw-hero"
          >
            <BlockBody block={data.vocabulary} onRetry={onRetry}>
              {(v) => (
                <div className="vg-figure">
                  <span className="vg-number">
                    {formatNumber(v.distinct, locale)}
                  </span>
                  <span className="vg-unit">
                    {t(
                      `voxa.words.vocabulary.unit.${pluralForm(v.distinct, locale)}`,
                    )}
                    <span className="vg-muted vgw-of">
                      {t("voxa.words.vocabulary.of", {
                        total: formatNumber(v.total, locale),
                      })}
                    </span>
                  </span>
                </div>
              )}
            </BlockBody>
          </Card>

          <Card
            title={t("voxa.words.top.title")}
            hint={t("voxa.words.top.hint")}
            testId="words-top"
          >
            <BlockBody block={data.top} onRetry={onRetry}>
              {(words) =>
                words.length === 0 ? (
                  <Empty />
                ) : (
                  <>
                    <WordRows
                      words={showAll ? words : words.slice(0, COLLAPSED_ROWS)}
                      locale={locale}
                      onSelect={onSelect}
                      testId="top-words"
                    />
                    {words.length > COLLAPSED_ROWS && (
                      <button
                        type="button"
                        className="vg-pill-btn vgw-more"
                        onClick={() => setShowAll((v) => !v)}
                      >
                        {showAll
                          ? t("voxa.words.top.showLess")
                          : t("voxa.words.top.showAll", {
                              count: words.length,
                            })}
                      </button>
                    )}
                  </>
                )
              }
            </BlockBody>
          </Card>

          <Card
            title={t("voxa.words.openers.title")}
            hint={t("voxa.words.openers.hint")}
            testId="words-openers"
          >
            <BlockBody block={data.openers} onRetry={onRetry}>
              {(o) =>
                o.one.length === 0 ? (
                  <Empty />
                ) : (
                  <div className="vgw-two">
                    <div>
                      <h4 className="vg-eyebrow">
                        {t("voxa.words.openers.one")}
                      </h4>
                      <TextRows
                        rows={o.one.map((r: OpenerStat) => r)}
                        locale={locale}
                        testId="openers-one"
                      />
                    </div>
                    <div>
                      <h4 className="vg-eyebrow">
                        {t("voxa.words.openers.two")}
                      </h4>
                      <TextRows
                        rows={o.two.map((r: OpenerStat) => r)}
                        locale={locale}
                        testId="openers-two"
                      />
                    </div>
                  </div>
                )
              }
            </BlockBody>
          </Card>

          <Card
            title={t("voxa.words.phrases.title")}
            hint={t("voxa.words.phrases.hint")}
            testId="words-phrases"
          >
            <BlockBody block={data.phrases} onRetry={onRetry}>
              {(p) =>
                p.bigrams.length === 0 && p.trigrams.length === 0 ? (
                  <Empty />
                ) : (
                  <div className="vgw-two">
                    <div>
                      <h4 className="vg-eyebrow">
                        {t("voxa.words.phrases.two")}
                      </h4>
                      <TextRows
                        rows={p.bigrams.map((r: PhraseStat) => r)}
                        locale={locale}
                        testId="phrases-two"
                      />
                    </div>
                    <div>
                      <h4 className="vg-eyebrow">
                        {t("voxa.words.phrases.three")}
                      </h4>
                      <TextRows
                        rows={p.trigrams.map((r: PhraseStat) => r)}
                        locale={locale}
                        testId="phrases-three"
                      />
                    </div>
                  </div>
                )
              }
            </BlockBody>
          </Card>

          <Card
            title={t("voxa.words.fillers.title")}
            hint={t("voxa.words.fillers.hint")}
            testId="words-fillers"
          >
            <BlockBody block={data.fillers} onRetry={onRetry}>
              {(f) =>
                f.items.length === 0 ? (
                  <p className="vg-muted vg-empty-line">
                    {t("voxa.words.fillers.none")}
                  </p>
                ) : (
                  <>
                    <ul className="vgw-rows" data-testid="filler-list">
                      {f.items.map((item: FillerStat) => (
                        <li
                          key={item.word}
                          className="vgw-row is-static is-filler"
                        >
                          <span className="vgw-row-label">{item.word}</span>
                          <Sparkline values={item.weekly} />
                          <span className="vgw-row-count vg-tabular">
                            {formatNumber(item.count, locale)}
                          </span>
                        </li>
                      ))}
                    </ul>
                    <TrendLine weeks={f.weeks} locale={locale} />
                  </>
                )
              }
            </BlockBody>
          </Card>

          <Card
            title={t("voxa.words.newWords.title")}
            hint={t("voxa.words.newWords.hint")}
            testId="words-new"
          >
            <BlockBody block={data.fresh} onRetry={onRetry}>
              {(words) =>
                words.length === 0 ? (
                  <Empty />
                ) : (
                  <ul className="vgw-chips" data-testid="new-words">
                    {words.map((w) => (
                      <li key={w.stem}>
                        <button
                          type="button"
                          className="vgw-chip"
                          onClick={() =>
                            onSelect({ stem: w.stem, word: w.word })
                          }
                        >
                          {w.word}
                          <span className="vg-muted vg-tabular">
                            {formatNumber(w.count, locale)}
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )
              }
            </BlockBody>
          </Card>
        </>
      )}

      {selected && (
        <WordDetailPanel
          word={selected.word}
          detail={detail}
          locale={locale}
          onClose={() => onSelect(null)}
        />
      )}
    </div>
  );
};

export default WordsView;
