import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ShortcutInput } from "../ShortcutInput";
import { PushToTalk } from "../PushToTalk";
import { useSettings } from "../../../hooks/useSettings";

export const ShortcutsSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const pushToTalk = getSetting("push_to_talk");
  const isLinux = type() === "linux";

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div>
        <h1 className="text-xl font-semibold mb-2">
          {t("settings.shortcuts.title")}
        </h1>
        <p className="text-sm text-text/60">
          {t("settings.shortcuts.description")}
        </p>
      </div>
      <SettingsGroup>
        <ShortcutInput shortcutId="transcribe" grouped={true} />
        <PushToTalk descriptionMode="tooltip" grouped={true} />
        {!isLinux && !pushToTalk && (
          <ShortcutInput shortcutId="cancel" grouped={true} />
        )}
        <ShortcutInput
          shortcutId="transcribe_with_post_process"
          grouped={true}
        />
      </SettingsGroup>
    </div>
  );
};
