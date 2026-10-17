import React from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, RotateCw, Trash2 } from "lucide-react";
import type {
  DictionaryCandidate,
  ModelStats,
  PeriodStats,
  VoxaStats,
} from "@/bindings";
import { VoxaMark } from "../brand/VoxaMark";
import { DailyBars } from "./DailyBars";
import {
  MIN_LATENCY_SAMPLES,
  dayLabel,
  formatNumber,
  formatPercent,
  pluralForm,
  splitDuration,
  splitMinutes,
  splitModelId,
} from "./format";

export interface StatsViewProps {
  stats: VoxaStats;
  locale: string;
  /** Term whose add request is in flight. */
  pendingTerm: string | null;
  onAddWord: (term: string) => void;
  onClear: () => void;
  onRefresh: () => void;
}

/** "6 420" + the unit in the right plural form, then the supporting line. */
const PeriodCard: React.FC<{
  title: string;
  period: PeriodStats;
  locale: string;
}> = ({ title, period, locale }) => {
  const { t } = useTranslation();
  const minutes = splitMinutes(period.minutes);
  return (
    <section className="vg-card vg-stat">
      <h3 className="vg-card-title">{title}</h3>
      <div className="vg-figure">
        <span className="vg-number">{formatNumber(period.words, locale)}</span>
        <span className="vg-unit">
          {t(`voxa.stats.units.words.${pluralForm(period.words, locale)}`)}
        </span>
      </div>
      <p className="vg-muted vg-stat-foot">
        {t("voxa.stats.cards.dictations", {
          value: formatNumber(period.dictations, locale),
        })}
        {" · "}
        {t("voxa.stats.cards.minutes", {
          value: formatNumber(minutes.value, locale),
          unit: t(
            minutes.unit === "h"
              ? "voxa.stats.units.hour"
              : "voxa.stats.units.min",
          ),
        })}
      </p>
    </section>
  );
};

const LatinMeter: React.FC<{ share: number | null }> = ({ share }) => (
  <div
    className="vg-meter"
    role="meter"
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={share === null ? undefined : Math.round(share * 100)}
  >
    <div
      className="vg-meter-fill"
      style={{ width: `${Math.round((share ?? 0) * 100)}%` }}
    />
  </div>
);

const LatencyCell: React.FC<{ model: ModelStats }> = ({ model }) => {
  const { t } = useTranslation();
  if (!model.enough_latency_data) {
    return (
      <span
        className="vg-muted"
        title={t("voxa.stats.models.notEnoughHint", {
          min: MIN_LATENCY_SAMPLES,
          count: model.latency_samples,
        })}
      >
        {t("voxa.stats.models.notEnough")}
      </span>
    );
  }
  const unit = (ms: number | null) => {
    if (ms === null) return "—";
    const d = splitDuration(ms);
    return `${d.value} ${t(d.unit === "ms" ? "voxa.stats.units.ms" : "voxa.stats.units.sec")}`;
  };
  return (
    <span className="vg-tabular">
      {unit(model.avg_latency_ms)} / {unit(model.p95_latency_ms)}
    </span>
  );
};

const ModelName: React.FC<{ modelId: string }> = ({ modelId }) => {
  const { name, quant } = splitModelId(modelId);
  return (
    <>
      <span className="vg-model-name">{name}</span>
      {quant && <span className="vg-model-quant">{quant}</span>}
    </>
  );
};

const ModelsTable: React.FC<{ models: ModelStats[]; locale: string }> = ({
  models,
  locale,
}) => {
  const { t } = useTranslation();
  if (models.length === 0) return null;
  return (
    <section className="vg-card vg-table-card">
      <h3 className="vg-card-title">{t("voxa.stats.models.title")}</h3>
      <p className="vg-muted">{t("voxa.stats.models.latencyHint")}</p>
      <div className="vg-table-wrap">
        <table className="vg-table">
          <thead>
            <tr>
              <th scope="col">{t("voxa.stats.models.columns.model")}</th>
              <th scope="col" className="num">
                {t("voxa.stats.models.columns.dictations")}
              </th>
              <th scope="col" className="num">
                {t("voxa.stats.models.columns.words")}
              </th>
              <th scope="col" className="num">
                {t("voxa.stats.models.columns.latency")}
              </th>
              <th scope="col" className="num">
                {t("voxa.stats.models.columns.latin")}
              </th>
            </tr>
          </thead>
          <tbody>
            {models.map((model) => (
              <tr key={model.model_id || "imported"}>
                <th scope="row" title={model.model_id}>
                  {model.model_id ? (
                    <ModelName modelId={model.model_id} />
                  ) : (
                    t("voxa.stats.models.imported")
                  )}
                </th>
                <td className="num vg-tabular">
                  {formatNumber(model.dictations, locale)}
                </td>
                <td className="num vg-tabular">
                  {formatNumber(model.words, locale)}
                </td>
                <td className="num">
                  <LatencyCell model={model} />
                </td>
                <td className="num vg-tabular">
                  {formatPercent(model.latin_share)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
};

const CandidateRow: React.FC<{
  candidate: DictionaryCandidate;
  pending: boolean;
  onAdd: (term: string) => void;
}> = ({ candidate, pending, onAdd }) => {
  const { t } = useTranslation();
  return (
    <li className="vg-candidate">
      <div className="vg-candidate-text">
        <div className="vg-candidate-term">
          <strong>{candidate.term}</strong>
          <span className="vg-muted vg-tabular">
            {t("voxa.stats.candidates.times", { count: candidate.count })}
          </span>
        </div>
        {candidate.variants.length > 0 && (
          <div className="vg-muted vg-candidate-variants">
            {t("voxa.stats.candidates.seen", {
              variants: candidate.variants.join(", "),
            })}
          </div>
        )}
      </div>
      {candidate.in_dictionary ? (
        <span className="vg-chip">
          {t("voxa.stats.candidates.inDictionary")}
        </span>
      ) : (
        <button
          type="button"
          className="vg-pill-btn"
          disabled={pending}
          onClick={() => onAdd(candidate.term)}
        >
          {t("voxa.stats.candidates.add")}
        </button>
      )}
    </li>
  );
};

const CandidateGroup: React.FC<{
  title: string;
  hint: string;
  items: DictionaryCandidate[];
  pendingTerm: string | null;
  onAdd: (term: string) => void;
}> = ({ title, hint, items, pendingTerm, onAdd }) => {
  const { t } = useTranslation();
  return (
    <section className="vg-card vg-candidates">
      <h3 className="vg-card-title">{title}</h3>
      <p className="vg-muted">{hint}</p>
      {items.length === 0 ? (
        <p className="vg-muted vg-empty-line">
          {t("voxa.stats.candidates.empty")}
        </p>
      ) : (
        <ul>
          {items.map((candidate) => (
            <CandidateRow
              key={`${candidate.kind}:${candidate.term}`}
              candidate={candidate}
              pending={pendingTerm === candidate.term}
              onAdd={onAdd}
            />
          ))}
        </ul>
      )}
    </section>
  );
};

export const EmptyStats: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className="vg-card vg-empty">
      <VoxaMark width={44} />
      <h3 className="vg-card-title">{t("voxa.stats.empty.title")}</h3>
      <p className="vg-muted">{t("voxa.stats.empty.body")}</p>
    </div>
  );
};

/** The statistics screen for a computed `VoxaStats`. No data fetching here. */
export const StatsView: React.FC<StatsViewProps> = ({
  stats,
  locale,
  pendingTerm,
  onAddWord,
  onClear,
  onRefresh,
}) => {
  const { t } = useTranslation();
  const isEmpty = stats.all_time.dictations === 0;
  const replaced = stats.candidates.filter((c) => c.kind === "replacement");
  const translit = stats.candidates.filter((c) => c.kind === "translit");
  const today = stats.daily[stats.daily.length - 1]?.date;
  const todayMinutes = splitMinutes(stats.today.minutes);

  return (
    <div className="vg-stats">
      <div className="vg-stats-top">
        <span className="vg-stats-date">
          {today ? dayLabel(today, locale) : t("voxa.stats.title")}
        </span>
        <div className="vg-stats-actions">
          <button
            type="button"
            className="vg-round-btn"
            aria-label={t("voxa.stats.refresh")}
            title={t("voxa.stats.refresh")}
            onClick={onRefresh}
          >
            <RotateCw aria-hidden="true" />
          </button>
          {!isEmpty && (
            <button
              type="button"
              className="vg-round-btn"
              aria-label={t("voxa.stats.footer.clear")}
              title={t("voxa.stats.footer.clear")}
              onClick={onClear}
            >
              <Trash2 aria-hidden="true" />
            </button>
          )}
        </div>
      </div>

      {isEmpty ? (
        <EmptyStats />
      ) : (
        <>
          <section className="vg-hero" aria-label={t("voxa.stats.cards.today")}>
            <div className="vg-figure">
              <span className="vg-number">
                {formatNumber(stats.today.words, locale)}
              </span>
              <span className="vg-unit">
                <AudioLines
                  className="vg-gold inline-block align-[-0.15em] me-1.5 w-[1.05em] h-[1.05em]"
                  aria-hidden="true"
                />
                {t(
                  `voxa.stats.hero.label.${pluralForm(stats.today.words, locale)}`,
                )}
              </span>
            </div>
            <p className="vg-hero-note">
              {t("voxa.stats.cards.dictations", {
                value: formatNumber(stats.today.dictations, locale),
              })}
              {" · "}
              {t("voxa.stats.cards.minutes", {
                value: formatNumber(todayMinutes.value, locale),
                unit: t(
                  todayMinutes.unit === "h"
                    ? "voxa.stats.units.hour"
                    : "voxa.stats.units.min",
                ),
              })}
            </p>
          </section>

          <div className="vg-grid-2">
            <PeriodCard
              title={t("voxa.stats.cards.week")}
              period={stats.week}
              locale={locale}
            />
            <PeriodCard
              title={t("voxa.stats.cards.allTime")}
              period={stats.all_time}
              locale={locale}
            />
          </div>

          <div className="vg-grid-2">
            <section className="vg-card vg-stat">
              <h3 className="vg-card-title">
                {t("voxa.stats.cards.avgWords")}
              </h3>
              <div className="vg-figure">
                <span className="vg-number">
                  {formatNumber(
                    Math.round(stats.avg_words_per_dictation * 10) / 10,
                    locale,
                  )}
                </span>
              </div>
              <p className="vg-muted vg-stat-foot">
                {t("voxa.stats.cards.avgSeconds")}:{" "}
                <span className="vg-tabular">
                  {stats.avg_seconds_per_dictation === null
                    ? "—"
                    : formatNumber(
                        Math.round(stats.avg_seconds_per_dictation * 10) / 10,
                        locale,
                      )}
                </span>
              </p>
            </section>
            <section className="vg-card vg-stat">
              <h3 className="vg-card-title">
                {t("voxa.stats.cards.latinShare")}
              </h3>
              <div className="vg-figure">
                <span className="vg-number">
                  {formatPercent(stats.latin_share)}
                </span>
              </div>
              <LatinMeter share={stats.latin_share} />
              <p className="vg-muted vg-stat-foot">
                {t("voxa.stats.cards.latinHint")}
              </p>
            </section>
          </div>

          <section className="vg-card">
            <DailyBars daily={stats.daily} locale={locale} />
          </section>

          <ModelsTable models={stats.models} locale={locale} />

          <div className="vg-section-head">
            <h3 className="vg-section-title">
              {t("voxa.stats.candidates.title")}
            </h3>
          </div>
          <CandidateGroup
            title={t("voxa.stats.candidates.replaced.title")}
            hint={t("voxa.stats.candidates.replaced.hint")}
            items={replaced}
            pendingTerm={pendingTerm}
            onAdd={onAddWord}
          />
          <CandidateGroup
            title={t("voxa.stats.candidates.translit.title")}
            hint={t("voxa.stats.candidates.translit.hint")}
            items={translit}
            pendingTerm={pendingTerm}
            onAdd={onAddWord}
          />
        </>
      )}

      <footer className="vg-stats-foot">
        <p className="vg-muted">{t("voxa.stats.footer.local")}</p>
      </footer>
    </div>
  );
};
