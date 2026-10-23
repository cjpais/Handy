import React, { useEffect, useRef, useState } from "react";
import { useSettings } from "@/hooks/useSettings";
import { useModelStore } from "@/stores/modelStore";
import { clearOnboardingProgress, useOnboardingStore } from "./onboardingStore";
import { usePermissions } from "./usePermissions";
import { useOnboardingModels } from "./useOnboardingModels";
import { Permissions } from "./screens/Permissions";
import { Model } from "./screens/Model";

/**
 * Onboarding's screens on their own, for people who finished onboarding
 * before: Permissions when one was taken away, and Model when their model
 * needs replacing (ModelMigration's "Choose a different model").
 */

export const ReturningPermissions: React.FC<{ onComplete: () => void }> = ({
  onComplete,
}) => {
  const permissions = usePermissions(false);
  if (!permissions.checked) return null;
  return <Permissions permissions={permissions} onContinue={onComplete} />;
};

export const ReplaceModel: React.FC<{
  onBack?: () => void;
  /** The new model is downloaded and in use. */
  onComplete: () => void;
}> = ({ onBack, onComplete }) => {
  const { settings } = useSettings();
  const store = useOnboardingStore();
  const models = useOnboardingModels(store.languages);
  const cancelDownload = useModelStore((s) => s.cancelDownload);
  const selectModel = useModelStore((s) => s.selectModel);
  // Switching to the model is under way, or failed: wait for the next pick
  // rather than retrying (a failed load re-renders this screen).
  const finishing = useRef(false);
  const [attempt, setAttempt] = useState(0);

  // Their languages, if onboarding recorded them.
  useEffect(() => {
    store.reset(false);
    const spoken = settings?.spoken_languages ?? [];
    store.set({
      ...(spoken.length ? { languages: spoken } : {}),
      modelId: null,
      extraModelId: null,
      downloadId: null,
    });
    // Once, on arrival.
  }, []);

  // Once the picked model is on disk: switch to it, and they're done.
  const picked = models.offered.find((m) => m.id === store.downloadId);
  const pickedInfo = picked && models.infoFor(picked);
  const pickedReady = models.downloadState(picked).state === "ready";
  useEffect(() => {
    if (finishing.current || !pickedInfo || !pickedReady) return;
    finishing.current = true;
    selectModel(pickedInfo.id).then((ok) => {
      if (!ok) return;
      // This was never a first run: leave no onboarding progress behind.
      clearOnboardingProgress();
      onComplete();
    });
  }, [pickedInfo, pickedReady, attempt, selectModel, onComplete]);

  return (
    <Model
      models={models}
      onBack={onBack}
      onChoose={(id) => {
        const model = models.offered.find((m) => m.id === id);
        const info = model && models.infoFor(model);
        if (!model || !info) return;
        const previous = models.offered.find((m) => m.id === store.downloadId);
        const previousInfo = previous && models.infoFor(previous);
        if (
          previousInfo &&
          previousInfo.id !== info.id &&
          models.downloadState(previous).state === "downloading"
        )
          cancelDownload(previousInfo.id);
        if (
          !info.is_downloaded &&
          (store.downloadId !== id ||
            models.downloadState(model).state === "failed")
        )
          models.startDownload(model);
        finishing.current = false;
        setAttempt((n) => n + 1);
        store.set({
          modelId: id,
          downloadId: id,
        });
      }}
    />
  );
};
