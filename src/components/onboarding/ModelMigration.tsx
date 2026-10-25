import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { RetiredModel } from "@/bindings";
import { Button } from "../ui/Button";
import { Radio } from "../ui/Radio";
import { Spinner } from "../ui/Spinner";
import { TextLink } from "../ui/TextLink";
import { DoneMark } from "../shared/DoneMark";
import { ProgressRing } from "../shared/ProgressRing";
import { Tag } from "../shared/Tag";
import { OnboardingShell, ScreenHeader, ScreenMain } from "./OnboardingShell";
import { useDownloadWords } from "./DownloadStatus";
import { useModelStore } from "../../stores/modelStore";
import { BYTES_PER_MB, formatModelSize } from "../../lib/utils/format";
import { commands } from "@/bindings";
import {
  deleteRetiredModel,
  findReplacement,
  type ModelMigrationPair,
} from "@/lib/models/retired";

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
  const { t } = useTranslation();
  const words = useDownloadWords();
  const {
    models,
    loading,
    downloadingModels,
    verifyingModels,
    downloadProgress,
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

  // Combined speed and ETA; none until every active download has a measured rate.
  let rate = 0;
  let remainingBytes = 0;
  let measured = true;
  for (const p of chosen) {
    const state = rowState(p);
    if (typeof state !== "number" || state >= 100) continue;
    const progress = downloadProgress[p.replacement.id];
    if (progress?.bytes_per_sec == null) measured = false;
    rate += progress?.bytes_per_sec ?? 0;
    remainingBytes +=
      progress && progress.total > 0
        ? progress.total - progress.downloaded
        : Number(p.replacement.size_mb) * BYTES_PER_MB * (1 - state / 100);
  }
  const timeLeft = words.time(
    measured && rate > 0 && remainingBytes > 0 ? remainingBytes / rate : null,
  );

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

  if (!snapshot) return null;

  const showChecks = rows.length > 1;
  // Nothing ticked: the only way on is the picker.
  const noWayForward = chosen.length === 0;
  const migrating = phase === "migrating";
  const working = migrating && (downloading || !targetFailed);

  const primary =
    phase === "choose" ? (
      noWayForward ? (
        <Button variant="primary" size="lg" onClick={onChooseAnother}>
          {t("onboarding.migration.chooseAnother")}
        </Button>
      ) : (
        <Button variant="primary" size="lg" onClick={handleStart}>
          {toDownload.length > 0
            ? t("onboarding.migration.download", {
                size: formatModelSize(downloadMb),
              })
            : t("onboarding.migration.continue")}
        </Button>
      )
    ) : targetFailed && !downloading ? (
      <Button variant="primary" size="lg" onClick={onChooseAnother}>
        {t("onboarding.migration.chooseAnother")}
      </Button>
    ) : undefined;

  const hint = working ? (
    <span className="inline-flex items-center gap-[8px]" aria-live="polite">
      <Spinner />
      {loadingTarget &&
        t("modelSelector.loading", { modelName: target.replacement.name })}
      {timeLeft && (
        <span className="tabular-nums">
          {t("onboarding.download.timeLeft", { time: timeLeft })}
        </span>
      )}
      {rate > 0 && <span className="tabular-nums">{words.speed(rate)}</span>}
    </span>
  ) : undefined;

  return (
    <OnboardingShell
      left={
        phase === "choose" && !noWayForward ? (
          <TextLink onClick={onChooseAnother}>
            {t("onboarding.migration.chooseAnother")}
          </TextLink>
        ) : undefined
      }
      hint={hint}
      primary={primary}
    >
      <ScreenMain
        layout="top"
        header={
          <ScreenHeader
            title={t("onboarding.migration.title")}
            sub={t("onboarding.migration.description")}
          />
        }
      >
        <ul className="mb-0 flex min-h-0 w-full flex-col overflow-y-auto rounded-panel border-[1.5px] border-line-strong px-[22px] py-[4px] text-start">
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
      </ScreenMain>
    </OnboardingShell>
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

/** One model to update: its replacement, what it replaces, and its state. */
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
            variant="outline"
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
        <span role="img" aria-label={t("modelSelector.modelReady")}>
          <DoneMark />
        </span>
      );
    }
    if (typeof state === "number") {
      return (
        <>
          <span className="tabular-nums">{Math.round(state)}%</span>
          <ProgressRing value={state / 100} size={22} />
        </>
      );
    }
    return (
      <span className="tabular-nums">
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
      className={`flex items-center gap-[14px] py-[14px] [&+&]:border-t [&+&]:border-line ${
        toggleable ? "cursor-pointer" : ""
      }`}
    >
      {showCheck && (
        <Radio checked={chosen} className={toggleable ? "" : "opacity-55"} />
      )}
      <div className="min-w-0 flex-1">
        <p className="m-0 truncate font-display text-row font-[650] leading-[1.3]">
          {replacement.name}
        </p>
        <div className="flex items-center gap-[8px] text-small text-ink-2">
          <span className="truncate">
            {t("onboarding.migration.replaces", { name: retired.name })}
          </span>
          <Tag>{t("onboarding.migration.oldFormat")}</Tag>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-[8px] text-label text-ink-3">
        {status()}
      </div>
    </li>
  );
};

export default ModelMigration;
