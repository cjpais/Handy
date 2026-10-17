import React from "react";
import { useTranslation } from "react-i18next";
import type {
  DictionaryCandidate,
  ModelStats,
  PeriodStats,
  VoxaStats,
} from "@/bindings";
import { Button } from "@/components/ui/Button";
import { VoxaMark } from "../brand/VoxaMark";
import { DailyBars } from "./DailyBars";
import {
  MIN_LATENCY_SAMPLES,
  formatNumber,
  formatPercent,
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
}

const PeriodCard: React.FC<{
  title: string;
  period: PeriodStats;
  locale: string;
}> = ({ title, period, locale }) => {
  const { t } = useTranslation();
  const minutes = splitMinutes(period.minutes);
  return (
    <section className="vg-card vg-stat">
      <h3 className="vg-eyebrow">{title}</h3>
      <div className="vg-number" aria-label={t("voxa.stats.cards.wordsLabel")}>
        {formatNumber(period.words, locale)}
      </div>
      <p className="vg-muted">
        {t("voxa.stats.cards.dictations", {
          value: formatNumber(period.dictations, locale),
        })}
      </p>
      <p className="vg-muted">
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
        <Button
          variant="primary-soft"
          size="sm"
          disabled={pending}
          onClick={() => onAdd(candidate.term)}
        >
          {t("voxa.stats.candidates.add")}
        </Button>
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
      <VoxaMark width={44} accent />
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
}) => {
  const { t } = useTranslation();
  const isEmpty = stats.all_time.dictations === 0;
  const replaced = stats.candidates.filter((c) => c.kind === "replacement");
  const translit = stats.candidates.filter((c) => c.kind === "translit");

  return (
    <div className="vg-stats">
      <header className="vg-stats-head">
        <h2 className="vg-title">{t("voxa.stats.title")}</h2>
        <p className="vg-muted">{t("voxa.stats.subtitle")}</p>
      </header>

      {isEmpty ? (
        <EmptyStats />
      ) : (
        <>
          <div className="vg-grid-3">
            <PeriodCard
              title={t("voxa.stats.cards.today")}
              period={stats.today}
              locale={locale}
            />
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
              <h3 className="vg-eyebrow">{t("voxa.stats.cards.avgWords")}</h3>
              <div className="vg-number">
                {formatNumber(
                  Math.round(stats.avg_words_per_dictation * 10) / 10,
                  locale,
                )}
              </div>
              <p className="vg-muted">
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
            <section
              className="vg-card vg-stat"
              title={t("voxa.stats.cards.latinHint")}
            >
              <h3 className="vg-eyebrow">{t("voxa.stats.cards.latinShare")}</h3>
              <div className="vg-number">
                {formatPercent(stats.latin_share)}
              </div>
              <LatinMeter share={stats.latin_share} />
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
        {!isEmpty && (
          <Button
            variant="danger-ghost"
            size="sm"
            className="shrink-0 whitespace-nowrap"
            onClick={onClear}
          >
            {t("voxa.stats.footer.clear")}
          </Button>
        )}
      </footer>
    </div>
  );
};
