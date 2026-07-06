import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { Keyboard, Mic, Sliders } from "lucide-react";
import { MicrophoneSelector } from "../MicrophoneSelector";
import { ShortcutInput } from "../ShortcutInput";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { OutputDeviceSelector } from "../OutputDeviceSelector";
import { PushToTalk } from "../PushToTalk";
import { OutputLanguageSelector } from "../OutputLanguageSelector";
import { AudioFeedback } from "../AudioFeedback";
import { useSettings } from "../../../hooks/useSettings";
import { VolumeSlider } from "../VolumeSlider";
import { MuteWhileRecording } from "../MuteWhileRecording";
import { ModelSettingsCard } from "./ModelSettingsCard";

export const GeneralSettings: React.FC = () => {
  const { t } = useTranslation();
  const { audioFeedbackEnabled, getSetting } = useSettings();
  const pushToTalk = getSetting("push_to_talk");
  const isLinux = type() === "linux";

  return (
    <div className="max-w-2xl w-full mx-auto space-y-6">
      {/* Header section with micro status */}
      <div className="flex justify-between items-end pb-4 border-b border-stone-mist/50">
        <div className="flex flex-col gap-1">
          <h1 className="text-xl font-bold font-cooper text-charcoal">
            {t("settings.general.title")}
          </h1>
          <p className="text-xs text-bark-grey">
            Manage your transcription hotkeys, recording behaviors, and hardware
            devices.
          </p>
        </div>
        <div className="flex items-center gap-1.5 px-2.5 py-1 bg-forest-green/10 border border-forest-green/20 rounded-tags select-none">
          <span className="w-1.5 h-1.5 rounded-full bg-forest-green animate-pulse" />
          <span className="text-[10px] font-mono font-bold tracking-wider text-forest-green uppercase">
            Active Profile
          </span>
        </div>
      </div>

      {/* Group 1: Hotkeys & Language */}
      <div className="relative group">
        <div className="absolute -inset-0.5 bg-gradient-to-r from-forest-green/20 to-tide-teal/10 rounded-cards blur-md opacity-25 group-hover:opacity-40 transition duration-300" />
        <div className="relative">
          <SettingsGroup
            title={
              <div className="flex items-center gap-2">
                <Keyboard className="w-4 h-4 text-forest-green" />
                <span>Shortcuts &amp; Language</span>
              </div>
            }
          >
            <ShortcutInput shortcutId="transcribe" grouped={true} />
            <PushToTalk descriptionMode="tooltip" grouped={true} />
            <OutputLanguageSelector descriptionMode="tooltip" grouped={true} />
            <ShortcutInput shortcutId="meeting" grouped={true} />
            {/* Cancel shortcut is hidden with push-to-talk (release key cancels) and on Linux (dynamic shortcut instability) */}
            {!isLinux && !pushToTalk && (
              <ShortcutInput shortcutId="cancel" grouped={true} />
            )}
          </SettingsGroup>
        </div>
      </div>

      {/* Model settings (renders only if supports translation/language selection) */}
      <ModelSettingsCard />

      {/* Group 2: Audio Hardware */}
      <div className="relative group">
        <div className="absolute -inset-0.5 bg-gradient-to-r from-terracotta/15 to-forest-green/10 rounded-cards blur-md opacity-20 group-hover:opacity-35 transition duration-300" />
        <div className="relative">
          <SettingsGroup
            title={
              <div className="flex items-center gap-2">
                <Mic className="w-4 h-4 text-terracotta" />
                <span>Audio Devices &amp; Levels</span>
              </div>
            }
          >
            <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
            <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
            <AudioFeedback descriptionMode="tooltip" grouped={true} />
            <OutputDeviceSelector
              descriptionMode="tooltip"
              grouped={true}
              disabled={!audioFeedbackEnabled}
            />
            <div className="p-4 bg-orange-off-white/30 rounded-inputs border border-stone-mist/40 space-y-2 mt-2">
              <div className="flex items-center gap-2 text-xs font-semibold text-bark-grey uppercase tracking-wider font-mono-tag">
                <Sliders className="w-3.5 h-3.5" />
                <span>Feedback Volume</span>
              </div>
              <VolumeSlider disabled={!audioFeedbackEnabled} />
            </div>
          </SettingsGroup>
        </div>
      </div>
    </div>
  );
};
