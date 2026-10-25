import React, { useEffect, useRef, useState } from "react";
import { useSettings } from "@/hooks/useSettings";
import { useModelStore } from "@/stores/modelStore";
import { clearOnboardingProgress, useOnboardingStore } from "./onboardingStore";
import { usePermissions } from "./usePermissions";
import { useOnboardingModels } from "./useOnboardingModels";
import { downloadPick } from "./downloadPick";
import { Permissions } from "./screens/Permissions";
import { Model } from "./screens/Model";

/** Onboarding screens reused later: a revoked permission, a retired model. */

export const ReturningPermissions: React.FC<{ onComplete: () => void }> = ({
  onComplete,
}) => {
  const permissions = usePermissions(false);
  if (!permissions.checked) return null;
  return <Permissions permissions={permissions} onContinue={onComplete} />;
};

export const ReplaceModel: React.FC<{
  onBack: () => void;
  /** Called once the new model is downloaded and in use. */
  onComplete: () => void;
}> = ({ onBack, onComplete }) => {
  const { settings } = useSettings();
  const store = useOnboardingStore();
  const models = useOnboardingModels(store.languages);
  const selectModel = useModelStore((s) => s.selectModel);
  // Don't retry a failed select; wait for the next pick.
  const finishing = useRef(false);
  const [attempt, setAttempt] = useState(0);

  // Seed with saved spoken languages, if any.
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

  const picked = models.offered.find((m) => m.id === store.downloadId);
  const pickedInfo = picked && models.infoFor(picked);
  const pickedReady = models.downloadState(picked).state === "ready";
  useEffect(() => {
    if (finishing.current || !pickedInfo || !pickedReady) return;
    finishing.current = true;
    selectModel(pickedInfo.id).then((ok) => {
      if (!ok) return;
      // Not a first run: leave no onboarding progress behind.
      clearOnboardingProgress();
      onComplete();
    });
  }, [pickedInfo, pickedReady, attempt, selectModel, onComplete]);

  return (
    <Model
      models={models}
      onBack={onBack}
      onChoose={(id) => {
        if (!downloadPick(models, id, store.downloadId)) return;
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
