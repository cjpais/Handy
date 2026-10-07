import React, { useEffect, useState, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import {
  STANDARD_GAMEPAD_BUTTON_MAP,
  formatGamepadCombination,
  sortGamepadButtons,
} from "../../lib/utils/gamepad";
import { ResetButton } from "../ui/ResetButton";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";
import { toast } from "sonner";

interface GamepadShortcutInputProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId?: string;
  disabled?: boolean;
}

export const GamepadShortcutInput: React.FC<GamepadShortcutInputProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  shortcutId = "transcribe_gamepad",
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateBinding, resetBinding, isUpdating } = useSettings();
  const [isRecording, setIsRecording] = useState(false);
  const [pressedButtons, setPressedButtons] = useState<string[]>([]);
  const [recordedButtons, setRecordedButtons] = useState<string[]>([]);
  const [originalBinding, setOriginalBinding] = useState<string>("");
  const containerRef = useRef<HTMLDivElement | null>(null);

  const bindings = getSetting("bindings") || {};
  const currentBindingObj = bindings[shortcutId] || {
    id: shortcutId,
    name: "Gamepad Transcribe Shortcut",
    description: "The gamepad shortcut to record and transcribe your voice.",
    default_binding: "select + start",
    current_binding: "select + start",
  };
  const currentBinding = currentBindingObj.current_binding || "select + start";

  const translatedName = t(
    `settings.general.shortcut.bindings.${shortcutId}.name`,
    currentBindingObj.name || "Gamepad Transcribe Shortcut",
  );
  const translatedDescription = t(
    `settings.general.shortcut.bindings.${shortcutId}.description`,
    currentBindingObj.description ||
      "The gamepad shortcut to record and transcribe your voice.",
  );

  const stopRecording = useCallback(async () => {
    setIsRecording(false);
    setPressedButtons([]);
    setRecordedButtons([]);
    setOriginalBinding("");
    await commands.resumeAllBindings().catch(console.error);
  }, []);

  // Gamepad polling loop while recording
  useEffect(() => {
    if (!isRecording) return;

    let animFrameId: number;
    let hadButtonsPressed = false;
    let capturedChord: Set<string> = new Set();

    const pollGamepad = () => {
      const gamepads =
        typeof navigator.getGamepads === "function"
          ? navigator.getGamepads()
          : [];

      const currentFrameButtons: string[] = [];

      for (let i = 0; i < gamepads.length; i++) {
        const gp = gamepads[i];
        if (!gp || !gp.connected) continue;

        for (let bIndex = 0; bIndex < gp.buttons.length; bIndex++) {
          const btn = gp.buttons[bIndex];
          if (btn && (btn.pressed || btn.value > 0.5)) {
            const btnName =
              STANDARD_GAMEPAD_BUTTON_MAP[bIndex] || `btn${bIndex}`;
            if (!currentFrameButtons.includes(btnName)) {
              currentFrameButtons.push(btnName);
            }
          }
        }
      }

      if (currentFrameButtons.length > 0) {
        hadButtonsPressed = true;
        for (const btn of currentFrameButtons) {
          capturedChord.add(btn);
        }
        setPressedButtons(currentFrameButtons);
        setRecordedButtons(Array.from(capturedChord));
      } else if (hadButtonsPressed && currentFrameButtons.length === 0) {
        // User has released all buttons in the chord -> commit shortcut!
        const finalButtons = sortGamepadButtons(Array.from(capturedChord));
        const newCombination = finalButtons.join(" + ");

        if (finalButtons.length > 0) {
          updateBinding(shortcutId, newCombination).catch((err) => {
            console.error("Failed to update gamepad binding:", err);
            toast.error(
              t("settings.general.shortcut.errors.set", {
                error: String(err),
              }),
            );
          });
        }
        stopRecording();
        return;
      }

      animFrameId = requestAnimationFrame(pollGamepad);
    };

    animFrameId = requestAnimationFrame(pollGamepad);

    return () => {
      cancelAnimationFrame(animFrameId);
    };
  }, [isRecording, shortcutId, updateBinding, stopRecording, t]);

  // Click outside and Escape key to cancel recording
  useEffect(() => {
    if (!isRecording) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        if (originalBinding) {
          updateBinding(shortcutId, originalBinding).catch(console.error);
        }
        stopRecording();
      }
    };

    const handleClickOutside = (e: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        if (originalBinding) {
          updateBinding(shortcutId, originalBinding).catch(console.error);
        }
        stopRecording();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("mousedown", handleClickOutside);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isRecording, originalBinding, shortcutId, updateBinding, stopRecording]);

  const startRecording = async () => {
    if (isRecording || disabled) return;
    await commands.suspendAllBindings().catch(console.error);
    setOriginalBinding(currentBinding);
    setPressedButtons([]);
    setRecordedButtons([]);
    setIsRecording(true);
  };

  return (
    <SettingContainer
      title={translatedName}
      description={translatedDescription}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={disabled}
      layout="horizontal"
    >
      <div ref={containerRef} className="flex items-center space-x-1">
        {isRecording ? (
          <div
            className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/30 rounded-md select-none"
            title={t(
              "settings.general.shortcut.pressGamepadButtons",
              "Press gamepad buttons...",
            )}
          >
            {recordedButtons.length > 0 ? (
              formatGamepadCombination(recordedButtons.join(" + "))
            ) : (
              <span className="text-mid-gray italic">
                {t(
                  "settings.general.shortcut.pressGamepadButtons",
                  "Press gamepad buttons...",
                )}
              </span>
            )}
          </div>
        ) : (
          <div
            className="px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 rounded-md cursor-pointer hover:border-logo-primary select-none transition-colors"
            onClick={startRecording}
          >
            {formatGamepadCombination(currentBinding)}
          </div>
        )}
        <ResetButton
          onClick={() => resetBinding(shortcutId)}
          disabled={isUpdating(`binding_${shortcutId}`) || disabled}
        />
      </div>
    </SettingContainer>
  );
};
