import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { StartHidden } from "../StartHidden";
import { AutostartToggle } from "../AutostartToggle";
import { ShowTrayIcon } from "../ShowTrayIcon";
import { UpdateChecksToggle } from "../UpdateChecksToggle";
import { ShowOverlay } from "../ShowOverlay";
import { ModelUnloadTimeoutSetting } from "../ModelUnloadTimeout";
import { AccelerationSelector } from "../AccelerationSelector";
import { ExperimentalToggle } from "../ExperimentalToggle";
import { KeyboardImplementationSelector } from "../debug/KeyboardImplementationSelector";
import { LazyStreamClose } from "../LazyStreamClose";
import { useSettings } from "../../../hooks/useSettings";

export const AppSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const experimentalEnabled = getSetting("experimental_enabled") || false;

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div>
        <h1 className="text-xl font-semibold mb-2">
          {t("settings.app.title")}
        </h1>
        <p className="text-sm text-text/60">{t("settings.app.description")}</p>
      </div>
      <SettingsGroup title={t("settings.app.startup.title")}>
        <StartHidden descriptionMode="tooltip" grouped={true} />
        <AutostartToggle descriptionMode="tooltip" grouped={true} />
        <ShowTrayIcon descriptionMode="tooltip" grouped={true} />
        <UpdateChecksToggle descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.app.display.title")}>
        <ShowOverlay descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.app.performance.title")}>
        <ModelUnloadTimeoutSetting descriptionMode="tooltip" grouped={true} />
        <AccelerationSelector descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.app.experimental.title")}>
        <ExperimentalToggle descriptionMode="tooltip" grouped={true} />
        {experimentalEnabled && (
          <>
            <KeyboardImplementationSelector
              descriptionMode="tooltip"
              grouped={true}
            />
            <LazyStreamClose descriptionMode="tooltip" grouped={true} />
          </>
        )}
      </SettingsGroup>
    </div>
  );
};
