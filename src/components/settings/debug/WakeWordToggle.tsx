import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { commands } from "../../../bindings";
import { listen } from "@tauri-apps/api/event";
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

    const [running, setRunning] = useState(false);
    const [updating, setUpdating] = useState(false);

    useEffect(() => {
        let cancelled = false;
        // Event listeners for backend wake-word logs
        const unsubs: Array<() => void> = [];
        (async () => {
            try {
                unsubs.push(
                    await listen<string>("wakeword-log", (e) => {
                        const msg = e.payload || "";
                        // Reflect backend state in UI to avoid polling delays
                        if (msg.includes("started")) {
                            setRunning(true);
                            setUpdating(false);
                        } else if (msg.includes("stopped")) {
                            setRunning(false);
                            setUpdating(false);
                        }
                    })
                );
                unsubs.push(
                    await listen<number>("wakeword-detected", (e) => {
                        // eslint-disable-next-line no-console
                        console.log("[wakeword-detected] p=", e.payload);
                    })
                );
                unsubs.push(
                    await listen<number>("wakeword-prob", (e) => {
                        // eslint-disable-next-line no-console
                        console.log("[wakeword-prob] p=", e.payload);
                    })
                );
            } catch { }
        })();
        (async () => {
            try {
                const res = await commands.isWakewordRunning();
                if (!cancelled) setRunning(res.status === "ok" ? !!res.data : false);
            } catch {
                if (!cancelled) setRunning(false);
            }
        })();
        return () => {
            cancelled = true;
            // Unsubscribe listeners
            unsubs.forEach((u) => {
                try {
                    u();
                } catch { }
            });
        };
    }, []);

    const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

    const onChange = async (enabled: boolean) => {
        // eslint-disable-next-line no-console
        console.log(`Wake-word toggled: enabled=${enabled}`);
        // Optimistically update so the toggle moves immediately
        setRunning(enabled);
        setUpdating(true);
        try {
            if (enabled) {
                const res = await commands.startWakeword(0.5);
                if (res.status !== "ok") {
                    console.log(`Wake-word start error: ${res.error}`);
                    setRunning(!enabled);
                } else {
                    // Rely on backend wakeword-log "started" event to update state;
                    // keep a soft fallback check after a generous grace period
                    setTimeout(async () => {
                        const s = await commands.isWakewordRunning();
                        if (!(s.status === "ok" && s.data === true)) {
                            console.log("Wake-word still initializing; backend may be loading models");
                        }
                    }, 15000);
                }
            } else {
                const res = await commands.stopWakeword();
                if (res.status !== "ok") {
                    console.log(`Wake-word stop error: ${res.error}`);
                    setRunning(!enabled);
                }
                // Give the backend a moment to detach callback
                await sleep(200);
                const confirm = await commands.isWakewordRunning();
                setRunning(confirm.status === "ok" ? !!confirm.data : false);
            }
        } catch (e) {
            console.log(`Wake-word toggle error: ${e}`);
            // Revert on failure
            setRunning(!enabled);
        } finally {
            setUpdating(false);
        }
    };

    return (
        <ToggleSwitch
            checked={running}
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
