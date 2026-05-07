import React from "react";
import { useTranslation } from "react-i18next";
import type { TranscriptionBackend } from "@/bindings";

interface LiveSttPrivacyNoticeProps {
  backend: TranscriptionBackend;
}

export const LiveSttPrivacyNotice: React.FC<LiveSttPrivacyNoticeProps> = ({
  backend,
}) => {
  const { t } = useTranslation();

  return (
    <div className="px-4 py-3 text-xs leading-relaxed text-mid-gray">
      {backend === "live_stt"
        ? t("settings.transcriptionBackend.privacy.livestt")
        : t("settings.transcriptionBackend.privacy.local")}
    </div>
  );
};
