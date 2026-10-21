import React, { useEffect, useLayoutEffect } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { recognitionLanguage } from "@/lib/constants/languages";
import {
  STEP_ORDER,
  clearOnboardingProgress,
  useOnboardingStore,
  type OnboardingStep,
} from "./onboardingStore";
import { usePermissions } from "./usePermissions";
import type { ChineseScriptChoice } from "./data/languages";
import { isChinese } from "./data/languages";
import { Welcome } from "./screens/Welcome";
import { Permissions } from "./screens/Permissions";
import { Model } from "./screens/Model";
import { Demo } from "./screens/Demo";
import { TryIt } from "./screens/TryIt";
import { AllSet } from "./screens/AllSet";
import { useOsType } from "@/hooks/useOsType";
import { OnboardingPill } from "./OnboardingPill";
import { useOnboardingModels } from "./useOnboardingModels";
import { useModelStore } from "@/stores/modelStore";

interface OnboardingFlowProps {
  /** Debug preview: inert (no prompts, downloads or setting writes). */
  preview?: boolean;
  /** The screen a preview opens on. */
  previewStep?: OnboardingStep;
  /** Called once the person closes the last screen. */
  onComplete: () => void;
}

// The backend's chinese_script values for the script picked on Welcome.
const SCRIPT_SETTING: Record<
  ChineseScriptChoice,
  "simplified" | "traditional"
> = {
  "zh-Hans": "simplified",
  "zh-Hant": "traditional",
  "zh-HK": "traditional",
};

/**
 * First-run onboarding: Welcome, Permissions (where the OS asks), Model,
 * Demo, Try it, All set. Remembers where it is, so a relaunch (the macOS
 * Accessibility fallback) or a quit comes back to the same screen.
 */
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
  const cancelDownload = useModelStore((s) => s.cancelDownload);
  const selectModel = useModelStore((s) => s.selectModel);

  // Once the model list is in (read from the store, not this render: the
  // reset below runs first). Has a model: one already on disk is the starting
  // pick (the selected model if it's on disk, else an on-disk pick, else any
  // on-disk model). Back mid-download after a quit: that download stopped.
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
    const chosen =
      models.catalogFor(settings?.selected_model ?? "") ??
      onDisk.find((m) => models.picks.some((p) => p.model.id === m.id)) ??
      onDisk[0];
    if (chosen?.id && models.infoFor(chosen)?.is_downloaded)
      store.set({ downloadId: chosen.id, downloadOnDisk: true });
    // Decided once the model list is in.
  }, [models.offered.length]);

  // The picked model becomes Handy's model as soon as it's on disk, so Try it
  // can dictate with it.
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

  // Before the first paint, so a preview never shows the saved step. Leaving
  // a preview puts the real progress back.
  useLayoutEffect(() => {
    store.reset(preview);
    if (preview && previewStep) store.set({ step: previewStep });
    return () => {
      if (preview) store.reset(false);
    };
    // Once per mount.
  }, []);

  // A preview can switch the menus' language on Welcome; leaving it puts
  // back the language the app was in.
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
  const go = (to: OnboardingStep) => store.set({ step: to });
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
    if (!preview) {
      await commands.completeOnboarding();
      await commands.closeMainWindow();
    }
    onComplete();
    // After the window has gone, so the flow doesn't flash back to Welcome.
    if (!preview) clearOnboardingProgress();
  };

  /** Download (or keep) the picked model, then move on. */
  const chooseModel = (id: string) => {
    startModel(id);
    next();
  };

  /** Make this the model: download it unless it's on disk already. */
  const startModel = (id: string) => {
    const model = models.offered.find((m) => m.id === id);
    const info = model && models.infoFor(model);
    if (!preview && model && info) {
      const previous = models.offered.find((m) => m.id === store.downloadId);
      const previousInfo = previous && models.infoFor(previous);
      if (
        previousInfo &&
        previousInfo.id !== info.id &&
        models.downloadState(previous).state === "downloading"
      )
        cancelDownload(previousInfo.id);
      // A new pick, or the same one again after its download stopped.
      if (
        !info.is_downloaded &&
        (store.downloadId !== id ||
          models.downloadState(model).state === "failed")
      )
        models.startDownload(model);
    }
    store.set({
      modelId: id,
      downloadId: id,
      downloadOnDisk:
        store.downloadId === id ? store.downloadOnDisk : !!info?.is_downloaded,
    });
  };
  const downloadNow = preview
    ? ({ state: "ready", onDisk: false } as const)
    : models.downloadState(downloaded);
  const modelReady = downloadNow.state === "ready";
  const pill =
    steps.indexOf(step) >= steps.indexOf("model") ? (
      <OnboardingPill models={models} />
    ) : undefined;

  const saveLanguages = () => {
    if (preview) return;
    const { languages, script } = useOnboardingStore.getState();
    updateSetting("spoken_languages", languages);
    updateSetting(
      "selected_language",
      languages.length === 1 ? recognitionLanguage(languages[0]) : "auto",
    );
    if (languages.some(isChinese))
      updateSetting("chinese_script", SCRIPT_SETTING[script]);
  };

  switch (step) {
    case "welcome":
      return (
        <Welcome
          progress={progress}
          preview={preview}
          onContinue={() => {
            saveLanguages();
            next();
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
          pill={pill}
          onBack={back}
          onChoose={chooseModel}
        />
      );
    case "demo":
      return (
        <Demo
          progress={progress}
          pill={pill}
          os={osType}
          ready={modelReady}
          downloadFailed={downloadNow.state === "failed"}
          onBack={back}
          onContinue={next}
        />
      );
    case "try":
      return (
        <TryIt
          progress={progress}
          pill={pill}
          os={osType}
          models={models}
          model={downloaded}
          download={downloadNow}
          onSwitch={startModel}
          onBack={back}
          onFinish={next}
        />
      );
    case "done":
      return (
        <AllSet
          progress={progress}
          pill={pill}
          os={osType}
          onBack={back}
          onClose={finish}
        />
      );
    default:
      return null;
  }
};
