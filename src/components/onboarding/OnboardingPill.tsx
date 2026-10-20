import React from "react";
import { useTranslation } from "react-i18next";
import { DownloadPill } from "../shared/DownloadPill";
import { TextLink } from "../ui/TextLink";
import { useOnboardingStore } from "./onboardingStore";
import type { useOnboardingModels } from "./useOnboardingModels";

/**
 * The download, top right from Model on: progress, checking, then ready; or
 * a retry if it stopped.
 */
export const OnboardingPill: React.FC<{
  models: ReturnType<typeof useOnboardingModels>;
}> = ({ models }) => {
  const { t } = useTranslation();
  const { downloadId, downloadOnDisk, preview } = useOnboardingStore();
  const model = models.offered.find((m) => m.id === downloadId);
  if (!model) return null;
  const state = preview
    ? { state: "ready" as const, onDisk: downloadOnDisk }
    : models.downloadState(model);
  const name = model.name;

  if (state.state === "none") return null;
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
    return (
      <DownloadPill
        state="ready"
        text={t(
          downloadOnDisk
            ? "onboarding.download.onDisk"
            : "onboarding.download.ready",
          { name },
        )}
      />
    );
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
