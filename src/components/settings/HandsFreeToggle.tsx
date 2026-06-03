import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { commands } from "../../bindings";

interface HandsFreeToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const HandsFreeToggle: React.FC<HandsFreeToggleProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();

  const [enabled, setEnabled] = useState(false);
  const [updating, setUpdating] = useState(false);

  // Reflect the current backend loop state on mount.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await commands.isHandsFreeRunning();
        if (!cancelled) setEnabled(res.status === "ok" ? !!res.data : false);
      } catch {
        if (!cancelled) setEnabled(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const onChange = async (next: boolean) => {
    setEnabled(next);
    setUpdating(true);
    try {
      // start/stop_hands_free persist the setting AND start/stop the loop.
      const res = next
        ? await commands.startHandsFree()
        : await commands.stopHandsFree();
      if (res.status !== "ok") setEnabled(!next);
    } catch {
      setEnabled(!next);
    } finally {
      setUpdating(false);
    }
  };

  return (
    <ToggleSwitch
      checked={enabled}
      onChange={onChange}
      isUpdating={updating}
      label={t("settings.advanced.handsFree.label")}
      description={t("settings.advanced.handsFree.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
