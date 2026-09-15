import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";

interface ChannelSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ChannelSelector: React.FC<ChannelSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating, isLoading } = useSettings();
    const [channelCount, setChannelCount] = useState(1);

    const selectedMicrophone = getSetting("selected_microphone") || "default";
    const selectedChannel = getSetting("selected_channel");

    useEffect(() => {
      let cancelled = false;
      setChannelCount(1);

      const fetchChannels = async () => {
        try {
          const deviceName =
            selectedMicrophone === "Default" ? "default" : selectedMicrophone;
          const result = await commands.getMicrophoneChannels(deviceName);
          if (!cancelled && result.status === "ok") {
            setChannelCount(result.data);
          }
        } catch (error) {
          console.error("Failed to get microphone channel count:", error);
        }
      };

      void fetchChannels();
      return () => {
        cancelled = true;
      };
    }, [selectedMicrophone]);

    // Don't render if the device only has one channel.
    if (channelCount <= 1) {
      return null;
    }

    const handleChannelSelect = async (value: string) => {
      const channel = value === "average" ? null : parseInt(value, 10);
      await updateSetting("selected_channel", channel);
    };

    const options = [
      { value: "average", label: t("settings.sound.channel.average") },
      ...Array.from({ length: channelCount }, (_, index) => ({
        value: index.toString(),
        label: t("settings.sound.channel.channel", { n: index + 1 }),
      })),
    ];

    // An old selection may not exist on a newly selected device. The recorder
    // also falls back to averaging in that case, so reflect that effective value.
    const currentValue =
      selectedChannel == null || selectedChannel >= channelCount
        ? "average"
        : selectedChannel.toString();

    return (
      <SettingContainer
        title={t("settings.sound.channel.title")}
        description={t("settings.sound.channel.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={currentValue}
          onSelect={handleChannelSelect}
          disabled={isUpdating("selected_channel") || isLoading}
        />
      </SettingContainer>
    );
  },
);

ChannelSelector.displayName = "ChannelSelector";
