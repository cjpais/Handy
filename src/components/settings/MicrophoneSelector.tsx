import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands, events } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { Button } from "../ui/Button";
import { useSettings } from "../../hooks/useSettings";

const isWindowHidden = () => document.visibilityState === "hidden";

interface MicrophoneSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MicrophoneSelector: React.FC<MicrophoneSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const {
      getSetting,
      updateSetting,
      resetSetting,
      isUpdating,
      isLoading,
      audioDevices,
      refreshAudioDevices,
    } = useSettings();
    const [microphoneTestSession, setMicrophoneTestSession] = useState<
      number | null
    >(null);
    const [microphoneLevel, setMicrophoneLevel] = useState(0);
    const [isChangingTestState, setIsChangingTestState] = useState(false);
    const microphoneTestSessionRef = useRef<number | null>(null);
    const mountedRef = useRef(true);
    const lifecycleGenerationRef = useRef(0);
    const startingRef = useRef(false);
    const stoppedWhileStartingRef = useRef(new Map<number, boolean>());
    const listenersReadyRef = useRef<Promise<unknown>>(Promise.resolve());

    useEffect(() => {
      mountedRef.current = true;
      const levelUnlisten = events.microphoneTestLevelEvent.listen((event) => {
        if (!mountedRef.current) return;
        if (event.payload.session_id === microphoneTestSessionRef.current) {
          setMicrophoneLevel(Math.max(0, Math.min(1, event.payload.level)));
        }
      });
      const stoppedUnlisten = events.microphoneTestStoppedEvent.listen(
        (event) => {
          if (!mountedRef.current) return;
          if (startingRef.current) {
            stoppedWhileStartingRef.current.set(
              event.payload.session_id,
              event.payload.failed ?? false,
            );
          }
          if (event.payload.session_id === microphoneTestSessionRef.current) {
            microphoneTestSessionRef.current = null;
            setMicrophoneTestSession(null);
            setMicrophoneLevel(0);
            if (event.payload.failed) {
              toast.error(t("settings.sound.microphone.testFailed"));
            }
          }
        },
      );

      const stopForHiddenWindow = () => {
        lifecycleGenerationRef.current += 1;
        const sessionId = microphoneTestSessionRef.current;
        microphoneTestSessionRef.current = null;
        if (mountedRef.current) {
          setMicrophoneTestSession(null);
          setMicrophoneLevel(0);
        }
        if (sessionId !== null) {
          void commands.stopMicrophoneTest(sessionId).catch(console.error);
        }
      };
      const onVisibilityChange = () => {
        if (isWindowHidden()) stopForHiddenWindow();
      };
      document.addEventListener("visibilitychange", onVisibilityChange);
      const closeUnlisten =
        getCurrentWindow().onCloseRequested(stopForHiddenWindow);
      listenersReadyRef.current = Promise.all([
        levelUnlisten,
        stoppedUnlisten,
        closeUnlisten,
      ]);

      return () => {
        mountedRef.current = false;
        stopForHiddenWindow();
        document.removeEventListener("visibilitychange", onVisibilityChange);
        for (const unlisten of [
          levelUnlisten,
          stoppedUnlisten,
          closeUnlisten,
        ]) {
          void unlisten.then((fn) => fn()).catch(console.error);
        }
      };
    }, [t]);

    const selectedMicrophone =
      getSetting("selected_microphone") === "default"
        ? "Default"
        : getSetting("selected_microphone") || "Default";

    const handleMicrophoneSelect = async (deviceName: string) => {
      await updateSetting("selected_microphone", deviceName);
    };

    const handleReset = async () => {
      await resetSetting("selected_microphone");
    };

    const handleMicrophoneTest = async () => {
      setIsChangingTestState(true);
      try {
        if (microphoneTestSession !== null) {
          const result = await commands.stopMicrophoneTest(
            microphoneTestSession,
          );
          if (result.status === "error") {
            toast.error(t("settings.sound.microphone.testFailed"));
            return;
          }
          microphoneTestSessionRef.current = null;
          setMicrophoneTestSession(null);
          setMicrophoneLevel(0);
          return;
        }

        const generation = lifecycleGenerationRef.current;
        const canAdoptSession = () =>
          mountedRef.current &&
          generation === lifecycleGenerationRef.current &&
          !isWindowHidden();
        startingRef.current = true;
        stoppedWhileStartingRef.current.clear();
        // Install event listeners before invoking: a backend stop can precede
        // the command response, including a disconnect during initialization.
        await listenersReadyRef.current;
        if (!canAdoptSession()) return;
        const result = await commands.startMicrophoneTest();
        if (result.status === "error") {
          toast.error(t("settings.sound.microphone.testFailed"));
          return;
        }
        if (!canAdoptSession()) {
          void commands.stopMicrophoneTest(result.data).catch(console.error);
          return;
        }
        if (stoppedWhileStartingRef.current.has(result.data)) {
          if (stoppedWhileStartingRef.current.get(result.data)) {
            toast.error(t("settings.sound.microphone.testFailed"));
          }
          return;
        }
        microphoneTestSessionRef.current = result.data;
        setMicrophoneTestSession(result.data);
        setMicrophoneLevel(0);
      } catch (error) {
        console.error("Failed to change microphone test state:", error);
        toast.error(t("settings.sound.microphone.testFailed"));
      } finally {
        startingRef.current = false;
        stoppedWhileStartingRef.current.clear();
        if (mountedRef.current) {
          setIsChangingTestState(false);
        }
      }
    };

    const microphoneOptions = audioDevices.map((device) => ({
      value: device.name,
      label: device.name,
    }));

    return (
      <SettingContainer
        title={t("settings.sound.microphone.title")}
        description={t("settings.sound.microphone.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex min-w-0 flex-col items-end gap-2">
          <div className="flex items-center space-x-1">
            <Dropdown
              options={microphoneOptions}
              selectedValue={selectedMicrophone}
              onSelect={handleMicrophoneSelect}
              placeholder={
                isLoading || audioDevices.length === 0
                  ? t("settings.sound.microphone.loading")
                  : t("settings.sound.microphone.placeholder")
              }
              disabled={
                isUpdating("selected_microphone") ||
                isLoading ||
                audioDevices.length === 0 ||
                microphoneTestSession !== null ||
                isChangingTestState
              }
              onRefresh={refreshAudioDevices}
            />
            <ResetButton
              onClick={handleReset}
              disabled={
                isUpdating("selected_microphone") ||
                isLoading ||
                microphoneTestSession !== null ||
                isChangingTestState
              }
            />
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="whitespace-nowrap"
              onClick={handleMicrophoneTest}
              aria-pressed={microphoneTestSession !== null}
              disabled={
                isChangingTestState ||
                (microphoneTestSession === null &&
                  (isLoading || audioDevices.length === 0))
              }
            >
              {microphoneTestSession === null
                ? t("settings.sound.microphone.test")
                : t("settings.sound.microphone.stopTest")}
            </Button>
          </div>
          {microphoneTestSession !== null && (
            <div className="w-full">
              <div className="mb-1 flex items-center justify-between text-xs text-text/70">
                <span>{t("settings.sound.microphone.inputLevel")}</span>
                <span>{Math.round(microphoneLevel * 100)}%</span>
              </div>
              <div
                className="h-2 w-full overflow-hidden rounded-full bg-mid-gray/20"
                role="meter"
                aria-label={t("settings.sound.microphone.inputLevel")}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(microphoneLevel * 100)}
              >
                <div
                  className="h-full bg-logo-primary transition-[width] duration-75"
                  style={{ width: `${microphoneLevel * 100}%` }}
                />
              </div>
            </div>
          )}
        </div>
      </SettingContainer>
    );
  },
);
