import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { commands } from "../../../bindings";
import { useSettings } from "../../../hooks/useSettings";

interface WakeWordToggleProps {
    descriptionMode?: "inline" | "tooltip";
    grouped?: boolean;
}

export const WakeWordToggle: React.FC<WakeWordToggleProps> = ({
    descriptionMode = "tooltip",
    grouped = false,
}) => {
    const { t } = useTranslation();
    const { getSetting } = useSettings();
    const alwaysOnMode = getSetting("always_on_microphone") || false;

    const [enabled, setEnabled] = useState(false);
    const [updating, setUpdating] = useState(false);

    // Simple initialization: reflect current backend flag state
    useEffect(() => {
        let cancelled = false;
        (async () => {
            try {
                const res = await commands.isWakewordRunning();
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
            if (next) {
                const res = await commands.startWakeword(0.5);
                if (res.status !== "ok") setEnabled(!next);
            } else {
                const res = await commands.stopWakeword();
                if (res.status !== "ok") setEnabled(!next);
            }
        } finally {
            setUpdating(false);
        }
    };

    return (
        <ToggleSwitch
            checked={enabled}
            onChange={onChange}
            disabled={!alwaysOnMode}
            isUpdating={updating}
            label={t("settings.debug.wakeword.label", { defaultValue: "Wake-word (debug)" })}
            description={t("settings.debug.wakeword.description", {
                defaultValue:
                    "Runs a local wake-word detector while the Always-On microphone is active.",
            })}
            descriptionMode={descriptionMode}
            grouped={grouped}
        />
    );
};
