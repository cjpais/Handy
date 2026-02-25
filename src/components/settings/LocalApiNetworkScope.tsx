import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { ApiNetworkScope } from "@/bindings";

interface LocalApiNetworkScopeProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const LocalApiNetworkScope: React.FC<LocalApiNetworkScopeProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const enabled = getSetting("local_api_enabled") ?? false;
  const value =
    (getSetting("local_api_network_scope") as ApiNetworkScope | undefined) ??
    "loopback";

  const options = [
    {
      value: "loopback" as ApiNetworkScope,
      label: t("settings.advanced.localApiNetwork.options.loopback"),
    },
    {
      value: "local_network" as ApiNetworkScope,
      label: t("settings.advanced.localApiNetwork.options.localNetwork"),
    },
  ];

  return (
    <SettingContainer
      title={t("settings.advanced.localApiNetwork.title")}
      description={t("settings.advanced.localApiNetwork.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={!enabled}
    >
      <Dropdown
        options={options}
        selectedValue={value}
        onSelect={(nextValue) =>
          updateSetting("local_api_network_scope", nextValue as ApiNetworkScope)
        }
        disabled={!enabled || isUpdating("local_api_network_scope")}
      />
    </SettingContainer>
  );
};
