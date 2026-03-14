import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { WordCorrectionThreshold } from "./WordCorrectionThreshold";
import { LogLevelSelector } from "./LogLevelSelector";
import { PasteDelay } from "./PasteDelay";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { AlwaysOnMicrophone } from "../AlwaysOnMicrophone";
import { SoundPicker } from "../SoundPicker";
import { ClamshellMicrophoneSelector } from "../ClamshellMicrophoneSelector";
import { ShortcutInput } from "../ShortcutInput";
import { UpdateChecksToggle } from "../UpdateChecksToggle";
import { useSettings } from "../../../hooks/useSettings";

export const DebugSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const pushToTalk = getSetting("push_to_talk");
  const isLinux = type() === "linux";

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.debug.title")}>
        <LogLevelSelector grouped={true} />
        <UpdateChecksToggle descriptionMode="tooltip" grouped={true} />
        <SoundPicker
          label={t("settings.debug.soundTheme.label")}
          description={t("settings.debug.soundTheme.description")}
        />
        <WordCorrectionThreshold descriptionMode="tooltip" grouped={true} />
        <PasteDelay descriptionMode="tooltip" grouped={true} />
        <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
        <ClamshellMicrophoneSelector descriptionMode="tooltip" grouped={true} />
        {/* Cancel shortcut is disabled on Linux due to instability with dynamic shortcut registration */}
        {!isLinux && (
          <ShortcutInput
            shortcutId="cancel"
            grouped={true}
            disabled={pushToTalk}
          />
        )}
      </SettingsGroup>
    </div>
  );
};
