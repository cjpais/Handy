import React, { useEffect, useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ChineseScript } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import {
  STEP_ORDER,
  clearOnboardingProgress,
  useOnboardingStore,
  type OnboardingStep,
} from "./onboardingStore";
import { usePermissions } from "./usePermissions";
import type { ChineseScriptChoice } from "@/lib/utils/languageSuggestions";
import { isChinese } from "@/lib/utils/languageSuggestions";
import { Welcome } from "./screens/Welcome";
import { Permissions } from "./screens/Permissions";
import { Model } from "./screens/Model";
import { Demo } from "./screens/Demo";
import { TryIt } from "./screens/TryIt";
import { useOsType } from "@/hooks/useOsType";
import { DownloadStatus } from "./DownloadStatus";
import { useOnboardingModels } from "./useOnboardingModels";
import { useModelStore } from "@/stores/modelStore";
import { moveScreens } from "./screenTransitions";
import { downloadPick } from "./downloadPick";
import "./screenTransitions.css";

interface OnboardingFlowProps {
  /** Debug preview: inert (no prompts, downloads or setting writes). */
  preview?: boolean;
  /** The screen a preview opens on. */
  previewStep?: OnboardingStep;
  /** Called after the last screen, to show Settings. */
  onComplete: () => void;
}

// The backend's chinese_script values for the script picked on Welcome.
const SCRIPT_SETTING: Record<ChineseScriptChoice, ChineseScript> = {
  "zh-Hans": "simplified",
  "zh-Hant": "traditional_tw",
  "zh-HK": "traditional_hk",
};

/** First-run onboarding. Progress persists, so a relaunch or quit resumes. */
export const OnboardingFlow: React.FC<OnboardingFlowProps> = ({
  preview = false,
  previewStep,
  onComplete,
}) => {
  const { i18n } = useTranslation();
  const store = useOnboardingStore();
  const { settings, updateSetting } = useSettings();
  const permissions = usePermissions(preview);
  const osType = useOsType();
  const models = useOnboardingModels(store.languages);
  const currentModel = useModelStore((s) => s.currentModel);
  const selectModel = useModelStore((s) => s.selectModel);

  // Once models load: start from an on-disk model if there is one, or flag a
  // download interrupted by a quit. Reads the store since reset() runs first.
  useEffect(() => {
    if (preview || !models.offered.length) return;
    const { downloadId } = useOnboardingStore.getState();
    if (downloadId) {
      const model = models.offered.find((m) => m.id === downloadId);
      if (model && models.downloadState(model).state === "none")
        store.set({ downloadFailed: true });
      return;
    }
    const onDisk = models.offered.filter(
      (m) => models.infoFor(m)?.is_downloaded,
    );
    if (!onDisk.length) return;
    const selected = models.catalogFor(settings?.selected_model ?? "");
    const chosen =
      onDisk.find((m) => m.id === selected?.id) ??
      onDisk.find((m) => models.picks.some((p) => p.model.id === m.id)) ??
      onDisk[0];
    store.set({ downloadId: chosen.id });
    // Decided once the model list is in.
  }, [models.offered.length]);

  // Select the picked model as soon as it's on disk, so Try it can use it.
  const downloaded = models.offered.find((m) => m.id === store.downloadId);
  const downloadedInfo = downloaded && models.infoFor(downloaded);
  useEffect(() => {
    if (preview || !downloadedInfo?.is_downloaded) return;
    if (currentModel !== downloadedInfo.id) selectModel(downloadedInfo.id);
  }, [
    preview,
    downloadedInfo?.id,
    downloadedInfo?.is_downloaded,
    currentModel,
    selectModel,
  ]);

  // Demo and Try it teach hold and tap, which only hold_or_toggle supports;
  // settings migrated from push_to_talk may still be Toggle or PushToTalk.
  const activationSet = useRef(false);
  const activation = settings?.shortcut_activation;
  useEffect(() => {
    if (preview || !activation || activationSet.current) return;
    activationSet.current = true;
    if (activation !== "hold_or_toggle")
      updateSetting("shortcut_activation", "hold_or_toggle");
  }, [preview, activation]);

  // Before first paint so a preview never shows the saved step.
  useLayoutEffect(() => {
    store.reset(preview);
    if (preview && previewStep) store.set({ step: previewStep });
    return () => {
      if (preview) store.reset(false);
    };
    // Once per mount.
  }, []);

  // Restore the UI language a preview may have changed on Welcome.
  useEffect(() => {
    if (!preview) return;
    const before = i18n.language;
    return () => {
      if (i18n.language !== before) i18n.changeLanguage(before);
    };
  }, [preview]);

  if (!permissions.checked) return null;

  const steps = STEP_ORDER.filter(
    (s) => s !== "permissions" || permissions.needed || store.welcomeBack,
  );
  const step = steps.includes(store.step) ? store.step : steps[0];
  const progress = { index: steps.indexOf(step), count: steps.length };
  // Reaching returnTo ends the detour.
  const go = (to: OnboardingStep) =>
    store.set(
      to === store.returnTo ? { step: to, returnTo: null } : { step: to },
    );
  /** Next screen, or back to All set after a detour from its panel. */
  const onward = () => (store.returnTo ? go(store.returnTo) : next());
  const next = () => {
    const i = steps.indexOf(step);
    if (i < steps.length - 1) go(steps[i + 1]);
    else finish();
  };

  const back = () => {
    const i = steps.indexOf(step);
    if (i > 0) go(steps[i - 1]);
  };

  const finish = async () => {
    // Saved only now: each login item change posts a macOS notification.
    if (!preview) {
      await updateSetting("autostart_enabled", store.atLogin);
      await commands.completeOnboarding();
    }
    onComplete();
    // After Settings replaces the flow, so Welcome doesn't flash.
    if (!preview) clearOnboardingProgress();
  };

  const chooseModel = (id: string) => {
    startModel(id);
    onward();
  };

  const startModel = (id: string) => {
    if (!preview) downloadPick(models, id, store.downloadId);
    store.set({
      modelId: id,
      downloadId: id,
    });
  };
  const downloadNow = preview
    ? ({ state: "ready" } as const)
    : models.downloadState(downloaded);
  const modelReady = downloadNow.state === "ready";

  const saveLanguages = () => {
    if (preview) return;
    const { languages, script } = useOnboardingStore.getState();
    // The model's own Language stays as is; effective_language uses these.
    updateSetting("spoken_languages", languages);
    if (languages.some(isChinese))
      updateSetting("chinese_script", SCRIPT_SETTING[script]);
  };

  switch (step) {
    case "welcome":
      return (
        <Welcome
          progress={progress}
          onContinue={() => {
            saveLanguages();
            onward();
          }}
        />
      );
    case "permissions":
      return (
        <Permissions
          permissions={permissions}
          progress={progress}
          onBack={back}
          onContinue={next}
        />
      );
    case "model":
      return (
        <Model
          progress={progress}
          models={models}
          onBack={store.returnTo ? () => go(store.returnTo!) : back}
          onChoose={chooseModel}
        />
      );
    case "demo":
      return (
        <Demo
          progress={progress}
          os={osType}
          ready={modelReady}
          downloadFailed={downloadNow.state === "failed"}
          download={<DownloadStatus models={models} model={downloaded} />}
          onRetry={() => downloaded && startModel(downloaded.id)}
          onBack={back}
          onContinue={() => moveScreens("in", next)}
        />
      );
    // Same element for both steps, so Try it stays mounted into All set.
    case "try":
    case "done":
      return (
        <TryIt
          key="try"
          progress={progress}
          os={osType}
          finished={step === "done"}
          // Falls back to the selected model in a preview.
          model={
            downloaded ?? models.catalogFor(settings?.selected_model ?? "")
          }
          ready={modelReady}
          download={<DownloadStatus models={models} model={downloaded} />}
          onChangeModel={() => go("model")}
          onEdit={(to) => store.set({ step: to, returnTo: "done" })}
          onBack={() => moveScreens("out", back)}
          onArrive={() => go("done")}
          onUnfinish={() => go("try")}
          onDone={finish}
        />
      );
  }
};
