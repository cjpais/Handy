import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Check, Loader2 } from "lucide-react";
import type { RetiredModel } from "@/bindings";
import HandyTextLogo from "../icons/HandyTextLogo";
import { Button } from "../ui/Button";
import { useModelStore } from "../../stores/modelStore";
import { formatModelSize } from "../../lib/utils/format";
import { commands } from "@/bindings";
import {
  deleteRetiredModel,
  findReplacement,
  type ModelMigrationPair,
} from "./retiredModels";

interface ModelMigrationProps {
  onComplete: () => void;
  // Escape hatch to the model picker when nothing is ticked or the update fails.
  onChooseAnother: () => void;
  // The persisted selection, which is a retired model on disk.
  inUseModelId: string;
}

type Phase = "choose" | "migrating";
type RowState = "idle" | "ready" | "failed" | number;
type Outcome = "done" | "failed";

const ModelMigration: React.FC<ModelMigrationProps> = ({
  onComplete,
  onChooseAnother,
  inUseModelId,
}) => {
  const { t, i18n } = useTranslation();
  const {
    models,
    loading,
    downloadingModels,
    verifyingModels,
    downloadProgress,
    downloadStats,
    downloadModel,
    selectModel,
  } = useModelStore();
  // The retired list is read once: deleting old files takes them off disk, and
  // rows must not vanish mid-flow.
  const [snapshot, setSnapshot] = useState<RetiredModel[] | null>(null);
  const [phase, setPhase] = useState<Phase>("choose");
  // Ticked rows (retired ids). Starts with the model in use; any row can be
  // unticked.
  const [picked, setPicked] = useState<Set<string>>(new Set());
  // Keyed by retired model id; unset while still in progress.
  const [outcomes, setOutcomes] = useState<Record<string, Outcome | undefined>>(
    {},
  );
  const finalizing = useRef<Set<string>>(new Set());
  const wrappingUp = useRef(false);

  const snapshotRequested = useRef(false);
  useEffect(() => {
    if (snapshotRequested.current || loading) return;
    snapshotRequested.current = true;
    commands
      .getRetiredModels()
      .then((retired) => {
        // The model in use always leads the list.
        retired.sort(
          (a, b) =>
            Number(b.id === inUseModelId) - Number(a.id === inUseModelId),
        );
        setSnapshot(retired);
        setPicked(new Set([inUseModelId]));
      })
      .catch((e) => {
        // Without the list there is nothing to update: fall back to the picker.
        console.error("Failed to list retired models:", e);
        onChooseAnother();
      });
  }, [loading, inUseModelId, onChooseAnother]);

  const rows: ModelMigrationPair[] = useMemo(
    () =>
      // Every replacement is in the bundled catalog, so this only drops a row
      // if RETIRED_ONNX_MODELS drifts from catalog.json.
      (snapshot ?? []).flatMap((retired) => {
        const replacement = findReplacement(models, retired);
        return replacement ? [{ retired, replacement }] : [];
      }),
    [snapshot, models],
  );

  const isChosen = (p: ModelMigrationPair) => picked.has(p.retired.id);

  const chosen = useMemo(
    () => rows.filter((p) => picked.has(p.retired.id)),
    [rows, picked],
  );
  // The replacement that becomes the selected model: the one for the model in
  // use if it's ticked, otherwise the first ticked row.
  const targetId = (
    chosen.find((p) => p.retired.id === inUseModelId) ?? chosen[0]
  )?.retired.id;
  const toDownload = chosen.filter((p) => !p.replacement.is_downloaded);
  const downloadMb = toDownload.reduce(
    (sum, p) => sum + Number(p.replacement.size_mb),
    0,
  );

  // Downloaded and verified: safe to switch to and to drop the old.
  const isReplacementReady = useCallback(
    (p: ModelMigrationPair) => {
      const id = p.replacement.id;
      return (
        p.replacement.is_downloaded &&
        !(id in downloadingModels) &&
        !(id in verifyingModels)
      );
    },
    [downloadingModels, verifyingModels],
  );

  const rowState = (p: ModelMigrationPair): RowState => {
    if (!isChosen(p) || phase === "choose") return "idle";
    const outcome = outcomes[p.retired.id];
    if (outcome) return outcome === "done" ? "ready" : "failed";
    const id = p.replacement.id;
    if (id in downloadingModels) return downloadProgress[id]?.percentage ?? 0;
    // Verifying, waiting on the model list to refresh, or being
    // switched to: the bytes are all here.
    return 100;
  };

  const downloading = chosen.some((p) => typeof rowState(p) === "number");
  const targetFailed =
    targetId !== undefined && outcomes[targetId] === "failed";
  // The target's bytes are in and it is being switched to (loaded).
  const target = chosen.find((p) => p.retired.id === targetId);
  const loadingTarget =
    phase === "migrating" &&
    !downloading &&
    target !== undefined &&
    !outcomes[target.retired.id] &&
    isReplacementReady(target);

  // Combined speed and time remaining across the parallel downloads.
  let speed = 0;
  let remainingMb = 0;
  for (const p of chosen) {
    const state = rowState(p);
    if (typeof state !== "number" || state >= 100) continue;
    const id = p.replacement.id;
    const progress = downloadProgress[id];
    speed += downloadStats[id]?.speed ?? 0;
    remainingMb +=
      progress && progress.total > 0
        ? (progress.total - progress.downloaded) / (1024 * 1024)
        : Number(p.replacement.size_mb) * (1 - state / 100);
  }
  const secondsLeft = speed > 0 ? remainingMb / speed : 0;
  // Unit formatting is localized by the browser ("3 min", "3 Min."), so only
  // the "remaining" wrapper needs translating.
  const timeLeft =
    secondsLeft > 0
      ? new Intl.NumberFormat(i18n.language, {
          style: "unit",
          unit: secondsLeft < 60 ? "second" : "minute",
          unitDisplay: "short",
        }).format(
          secondsLeft < 60
            ? Math.ceil(secondsLeft)
            : Math.ceil(secondsLeft / 60),
        )
      : null;

  const setOutcome = (retiredId: string, outcome?: Outcome) =>
    setOutcomes((prev) => ({ ...prev, [retiredId]: outcome }));

  const download = async (p: ModelMigrationPair) => {
    setOutcome(p.retired.id);
    // Failure toasts come from the store's model-download-failed listener.
    if (!(await downloadModel(p.replacement.id))) {
      setOutcome(p.retired.id, "failed");
    }
  };

  const handleStart = () => {
    setPhase("migrating");
    // All at once; ready-made replacements are picked up by the effect below.
    toDownload.forEach(download);
  };

  const handleRetry = (p: ModelMigrationPair) => {
    setOutcome(p.retired.id);
    finalizing.current.delete(p.retired.id);
    // A replacement that downloaded but couldn't be selected only needs the
    // finalize step again, which the effect re-runs once it isn't failed.
    if (!isReplacementReady(p)) download(p);
  };

  const toggle = (id: string) => {
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // Each ready replacement: switch to it if it's the target, then delete its
  // old version. The old file is only removed after this succeeds,
  // so a failure never leaves someone with neither version.
  useEffect(() => {
    if (phase !== "migrating") return;
    for (const p of chosen) {
      const retiredId = p.retired.id;
      if (
        finalizing.current.has(retiredId) ||
        outcomes[retiredId] === "failed" ||
        !isReplacementReady(p)
      ) {
        continue;
      }
      finalizing.current.add(retiredId);
      const replacementId = p.replacement.id;
      (async () => {
        if (retiredId === targetId && !(await selectModel(replacementId))) {
          toast.error(t("onboarding.errors.selectModel"));
          finalizing.current.delete(retiredId);
          setOutcome(retiredId, "failed");
          return;
        }
        await deleteRetiredModel(retiredId);
        setOutcome(retiredId, "done");
      })();
    }
  }, [phase, chosen, outcomes, targetId, isReplacementReady, selectModel, t]);

  // Wrap up once the target is switched to and every other pick has either
  // finished or failed. A failed extra keeps its old file; the Models page
  // notice can clean it up later. Unticked models are removed here.
  useEffect(() => {
    if (phase !== "migrating" || wrappingUp.current) return;
    if (targetId !== undefined && outcomes[targetId] !== "done") return;
    if (!chosen.every((p) => outcomes[p.retired.id])) return;
    wrappingUp.current = true;
    (async () => {
      for (const p of rows) {
        if (!picked.has(p.retired.id)) await deleteRetiredModel(p.retired.id);
      }
      onComplete();
    })();
  }, [phase, targetId, picked, outcomes, chosen, rows, onComplete]);

  if (!snapshot) {
    return (
      <div className="h-screen w-full flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-text/50" />
      </div>
    );
  }

  const showChecks = rows.length > 1;
  // Nothing ticked: the only way on is the picker.
  const noWayForward = chosen.length === 0;

  return (
    <div className="h-screen w-full flex flex-col p-6 gap-5 items-center justify-center">
      <HandyTextLogo width={160} className="shrink-0" />

      <div className="text-center max-w-md shrink-0">
        <h2 className="text-xl font-semibold text-text mb-1.5">
          {t("onboarding.migration.title")}
        </h2>
        <p className="text-sm text-text/70">
          {t("onboarding.migration.description")}
        </p>
      </div>

      <ul className="w-full max-w-[560px] min-h-0 overflow-y-auto rounded-xl border-2 border-mid-gray/20 divide-y divide-mid-gray/20">
        {rows.map((pair) => (
          <MigrationRow
            key={pair.retired.id}
            pair={pair}
            phase={phase}
            chosen={isChosen(pair)}
            showCheck={showChecks}
            state={rowState(pair)}
            onToggle={toggle}
            onRetry={handleRetry}
          />
        ))}
      </ul>

      <div className="w-full max-w-[560px] flex flex-col items-center shrink-0">
        {phase === "choose" ? (
          noWayForward ? (
            <Button
              variant="primary"
              size="lg"
              className="w-full"
              onClick={onChooseAnother}
            >
              {t("onboarding.migration.chooseAnother")}
            </Button>
          ) : (
            <Button
              variant="primary"
              size="lg"
              className="w-full"
              onClick={handleStart}
            >
              {toDownload.length > 0
                ? t("onboarding.migration.download", {
                    size: formatModelSize(downloadMb),
                  })
                : t("onboarding.migration.continue")}
            </Button>
          )
        ) : downloading || !targetFailed ? (
          <p
            className="flex items-center gap-2 h-10 text-sm text-text/70"
            aria-live="polite"
          >
            <Loader2 className="w-4 h-4 animate-spin" />
            {loadingTarget && (
              <span>
                {t("modelSelector.loading", {
                  modelName: target.replacement.name,
                })}
              </span>
            )}
            {timeLeft && (
              <span className="tabular-nums text-text/50">
                {t("onboarding.migration.timeRemaining", { time: timeLeft })}
              </span>
            )}
            {timeLeft && speed > 0 && (
              <span className="text-text/30" aria-hidden="true">
                ·
              </span>
            )}
            {speed > 0 && (
              <span className="tabular-nums text-text/50">
                {t("modelSelector.downloadSpeed", {
                  speed: speed.toFixed(1),
                })}
              </span>
            )}
          </p>
        ) : (
          <Button
            variant="ghost"
            size="md"
            className="h-10 text-text/70 hover:text-text"
            onClick={onChooseAnother}
          >
            {t("onboarding.migration.chooseAnother")}
          </Button>
        )}
      </div>
    </div>
  );
};

const ProgressRing: React.FC<{ progress: number }> = ({ progress }) => {
  const circumference = 2 * Math.PI * 9;
  return (
    <svg className="w-[22px] h-[22px] -rotate-90" viewBox="0 0 22 22">
      <circle
        cx="11"
        cy="11"
        r="9"
        fill="none"
        strokeWidth="2.5"
        className="stroke-mid-gray/25"
      />
      <circle
        cx="11"
        cy="11"
        r="9"
        fill="none"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeDasharray={circumference}
        strokeDashoffset={circumference * (1 - progress / 100)}
        className="stroke-logo-primary transition-[stroke-dashoffset] duration-150 ease-linear motion-reduce:transition-none"
      />
    </svg>
  );
};

interface MigrationRowProps {
  pair: ModelMigrationPair;
  phase: Phase;
  chosen: boolean;
  showCheck: boolean;
  state: RowState;
  onToggle: (id: string) => void;
  onRetry: (pair: ModelMigrationPair) => void;
}

const MigrationRow: React.FC<MigrationRowProps> = ({
  pair,
  phase,
  chosen,
  showCheck,
  state,
  onToggle,
  onRetry,
}) => {
  const { t } = useTranslation();
  const { retired, replacement } = pair;
  const oldName = retired.name;
  // Checkbox semantics only while choosing; later the row may hold a button.
  const toggleable = showCheck && phase === "choose";

  const status = () => {
    // Checked first: a replacement that downloaded but couldn't be selected
    // still needs its Retry button.
    if (state === "failed") {
      return (
        <>
          <span className="text-error">{t("onboarding.migration.failed")}</span>
          <Button
            variant="secondary"
            size="sm"
            onClick={(e) => {
              e.stopPropagation();
              onRetry(pair);
            }}
          >
            {t("onboarding.migration.retry")}
          </Button>
        </>
      );
    }
    // Already-downloaded picks are as good as ready before anything starts.
    if (state === "ready" || (chosen && replacement.is_downloaded)) {
      return (
        <span
          role="img"
          aria-label={t("modelSelector.modelReady")}
          className="grid place-items-center w-[22px] h-[22px] rounded-full bg-emerald-500/20 text-emerald-500"
        >
          <Check className="w-3 h-3" strokeWidth={3} />
        </span>
      );
    }
    if (typeof state === "number") {
      return (
        <>
          <span className="tabular-nums text-text/50">
            {Math.round(state)}%
          </span>
          <ProgressRing progress={state} />
        </>
      );
    }
    return (
      <span className="text-text/50">
        {formatModelSize(Number(replacement.size_mb))}
      </span>
    );
  };

  return (
    <li
      role={toggleable ? "checkbox" : undefined}
      aria-checked={toggleable ? chosen : undefined}
      tabIndex={toggleable ? 0 : undefined}
      onClick={() => toggleable && onToggle(retired.id)}
      onKeyDown={(e) => {
        if (toggleable && (e.key === " " || e.key === "Enter")) {
          e.preventDefault();
          onToggle(retired.id);
        }
      }}
      className={`flex items-center gap-3 px-4 py-3 ${
        toggleable
          ? "cursor-pointer hover:bg-mid-gray/5 focus-visible:outline-none focus-visible:bg-mid-gray/10"
          : ""
      }`}
    >
      {showCheck && (
        <span
          aria-hidden
          className={`grid place-items-center w-5 h-5 shrink-0 rounded-full border-2 transition-colors ${
            chosen
              ? "bg-background-ui border-background-ui text-white"
              : "border-mid-gray/45"
          } ${toggleable ? "" : "opacity-55"}`}
        >
          {chosen && <Check className="w-3 h-3" strokeWidth={3} />}
        </span>
      )}

      <div className="flex-1 min-w-0">
        <p className="text-sm font-semibold text-text truncate">
          {replacement.name}
        </p>
        <div className="flex items-center gap-2 text-xs text-text/40">
          <span className="truncate">
            {t("onboarding.migration.replaces", { name: oldName })}
          </span>
          <span className="shrink-0 rounded border border-mid-gray/35 px-1.5 text-[11px] font-medium text-text/55">
            {t("onboarding.migration.oldFormat")}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-2 shrink-0 text-xs">{status()}</div>
    </li>
  );
};

export default ModelMigration;
