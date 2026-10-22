import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { DownloadPill } from "../shared/DownloadPill";
import { TextLink } from "../ui/TextLink";
import { useOnboardingStore } from "./onboardingStore";
import type { useOnboardingModels } from "./useOnboardingModels";

// How long "ready" shows once a download finishes.
const READY_MS = 4000;

/**
 * The download, top right after Model: progress, checking, a retry if it
 * stopped, and "ready" for a few seconds as it finishes. A model that was
 * already on disk has no download to report.
 */
export const OnboardingPill: React.FC<{
  models: ReturnType<typeof useOnboardingModels>;
}> = ({ models }) => {
  const { t } = useTranslation();
  const { downloadId, preview } = useOnboardingStore();
  const model = models.offered.find((m) => m.id === downloadId);
  const state = preview
    ? ({ state: "none" } as const)
    : models.downloadState(model);

  // "Ready" is news only as the download finishes, not a lasting state.
  const [justReady, setJustReady] = useState(false);
  const loading = useRef(false);
  useEffect(() => {
    if (state.state === "downloading" || state.state === "checking") {
      loading.current = true;
      return;
    }
    if (state.state !== "ready" || !loading.current) return;
    loading.current = false;
    setJustReady(true);
    const timer = setTimeout(() => setJustReady(false), READY_MS);
    return () => clearTimeout(timer);
  }, [state.state]);

  if (!model || state.state === "none") return null;
  const name = model.name;
  if (state.state === "failed")
    return (
      <DownloadPill
        state="failed"
        text={t("onboarding.download.failed")}
        action={
          <TextLink
            className="py-0! text-label!"
            onClick={() => models.startDownload(model)}
          >
            {t("onboarding.download.retry")}
          </TextLink>
        }
      />
    );
  if (state.state === "checking")
    return (
      <DownloadPill state="checking" text={t("onboarding.download.checking")} />
    );
  if (state.state === "ready")
    return justReady ? (
      <DownloadPill
        state="ready"
        text={t("onboarding.download.ready", { name })}
      />
    ) : null;
  const eta = state.etaSeconds;
  const time =
    eta == null
      ? null
      : eta < 60
        ? t("onboarding.download.seconds", { count: Math.ceil(eta) })
        : t("onboarding.download.minutes", { count: Math.ceil(eta / 60) });
  return (
    <DownloadPill
      state="downloading"
      progress={state.progress}
      text={time ? t("onboarding.download.left", { name, time }) : name}
    />
  );
};
