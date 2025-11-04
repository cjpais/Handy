import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { PasteMethod } from "../../lib/types";

interface PasteMethodProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PasteMethodSetting: React.FC<PasteMethodProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [osType, setOsType] = useState<string>("unknown");

    useEffect(() => {
      setOsType(getOsType());
    }, []);

    const selectedMethod = (getSetting("paste_method") ||
      "ctrl_v") as PasteMethod;

    const pasteMethodOptions = useMemo(
      () => [
        {
          value: "ctrl_v",
          label: t("settings.debug.paste_method.options.ctrl_v"),
        },
        {
          value: "direct",
          label: t("settings.debug.paste_method.options.direct"),
        },
      ],
      [t],
    );

    return (
      <SettingContainer
        title={t("settings.debug.paste_method.title")}
        description={t("settings.debug.paste_method.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <Dropdown
          options={pasteMethodOptions}
          selectedValue={selectedMethod}
          onSelect={(value) =>
            updateSetting("paste_method", value as PasteMethod)
          }
          disabled={isUpdating("paste_method")}
        />
      </SettingContainer>
    );
  },
);
