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
    const { getSetting, isLoading } = useSettings();
    const [channelCount, setChannelCount] = useState<number>(1);
    const [selectedChannel, setSelectedChannel] = useState<number | null>(null);
    const [isUpdating, setIsUpdating] = useState(false);

    const selectedMicrophone = getSetting("selected_microphone") || "default";

    // Fetch channel count when the selected microphone changes
    useEffect(() => {
      const fetchChannels = async () => {
        const deviceName =
          selectedMicrophone === "Default" ? "default" : selectedMicrophone;
        const result = await commands.getMicrophoneChannels(deviceName);
        if (result.status === "ok") {
          setChannelCount(result.data);
        }
      };
      fetchChannels();
    }, [selectedMicrophone]);

    // Fetch the current selected channel setting
    useEffect(() => {
      const fetchSelectedChannel = async () => {
        const result = await commands.getSelectedChannel();
        if (result.status === "ok") {
          setSelectedChannel(result.data);
        }
      };
      fetchSelectedChannel();
    }, []);

    // Don't render if the device only has 1 channel
    if (channelCount <= 1) {
      return null;
    }

    const handleChannelSelect = async (value: string) => {
      setIsUpdating(true);
      const channel = value === "average" ? null : parseInt(value, 10);
      const result = await commands.setSelectedChannel(channel);
      if (result.status === "ok") {
        setSelectedChannel(channel);
      }
      setIsUpdating(false);
    };

    const options = [
      { value: "average", label: t("settings.sound.channel.average", "Average all channels") },
      ...Array.from({ length: channelCount }, (_, i) => ({
        value: i.toString(),
        label: t("settings.sound.channel.channel", "Channel {{n}}", { n: i + 1 }),
      })),
    ];

    const currentValue =
      selectedChannel === null || selectedChannel === undefined
        ? "average"
        : selectedChannel.toString();

    return (
      <SettingContainer
        title={t("settings.sound.channel.title", "Input Channel")}
        description={t(
          "settings.sound.channel.description",
          "Select which input channel to record from. Use this if your audio interface has multiple inputs."
        )}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={currentValue}
          onSelect={handleChannelSelect}
          disabled={isUpdating || isLoading}
        />
      </SettingContainer>
    );
  },
);

ChannelSelector.displayName = "ChannelSelector";
