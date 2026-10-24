import React, { useEffect, useLayoutEffect } from "react";
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
import type { ChineseScriptChoice } from "./data/languages";
import { isChinese } from "./data/languages";
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
  /** Called once the person leaves the last screen, for Settings. */
  onComplete: () => void;
}

// The backend's chinese_script values for the script picked on Welcome.
const SCRIPT_SETTING: Record<ChineseScriptChoice, ChineseScript> = {
  "zh-Hans": "simplified",
  "zh-Hant": "traditional_tw",
  "zh-HK": "traditional_hk",
};

/**
 * First-run onboarding: Welcome, Permissions (where the OS asks), Model,
 * Demo, Try it, and All set (Try it's screen, finished). Remembers where it
 * is, so a relaunch (the macOS Accessibility fallback) or a quit comes back
 * to the same screen.
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
      store.set({ downloadId: chosen.id });
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
  // Arriving where a panel row came from ends the detour.
  const go = (to: OnboardingStep) =>
    store.set(
      to === store.returnTo ? { step: to, returnTo: null } : { step: to },
    );
  /** On, or straight back to All set after a detour from its panel. */
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

  // Into Settings, not closed: the person may want to look around, and can
  // close the window themselves.
  const finish = async () => {
    // Set on All set, saved now: each change to the login item posts a macOS
    // notice, so toggling there doesn't register and unregister it.
    if (!preview) {
      await updateSetting("autostart_enabled", store.atLogin);
      await commands.completeOnboarding();
    }
    onComplete();
    // After Settings has replaced the flow, so it doesn't flash back to Welcome.
    if (!preview) clearOnboardingProgress();
  };

  /** Download (or keep) the picked model, then move on. */
  const chooseModel = (id: string) => {
    startModel(id);
    onward();
  };

  /** Make this the model: download it unless it's on disk already. */
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
    // The model's Language stays as it is (Auto, fresh): a model that can't
    // detect uses their one language from these at transcription time.
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
          // Try it is Demo's screen, close up: the camera moves in.
          onContinue={() => moveScreens("in", next)}
        />
      );
    // All set is Try it's screen, finished: the same element for both, so it
    // stays mounted (the note, its cursor, the live shortcut) as the glove
    // flies to the icon and the flow moves on underneath it.
    case "try":
    case "done":
      return (
        <TryIt
          key="try"
          progress={progress}
          os={osType}
          finished={step === "done"}
          // The model in use: the one picked here, else Handy's (a preview).
          model={
            downloaded ?? models.catalogFor(settings?.selected_model ?? "")
          }
          download={downloadNow}
          downloadStatus={<DownloadStatus models={models} model={downloaded} />}
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
