import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { RotateCcw } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

const DEFAULT_COLOR = "#faa2ca";

const stripHash = (hex: string) => hex.replace(/^#/, "");

const normalizeHex = (value: string): string | null => {
  const digits = value.replace(/^#+/, "");
  return /^[0-9a-fA-F]{6}$/.test(digits) ? `#${digits}` : null;
};

export const AccentColorPicker: React.FC<{
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}> = ({ descriptionMode = "tooltip", grouped = false }) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();

  const saved = getSetting("accent_color");
  const isDefault = !saved || !saved.startsWith("#");
  const currentColor = isDefault ? DEFAULT_COLOR : saved;

  const [hexInput, setHexInput] = useState(stripHash(currentColor));

  useEffect(() => {
    setHexInput(stripHash(currentColor));
  }, [currentColor]);

  const applyHex = (value: string) => {
    const hex = normalizeHex(value);
    if (hex) {
      setHexInput(stripHash(hex));
      updateSetting("accent_color", hex);
    } else {
      setHexInput(stripHash(currentColor));
    }
  };

  return (
    <SettingContainer
      title={t("settings.advanced.accentColor.label")}
      description={t("settings.advanced.accentColor.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => updateSetting("accent_color", "")}
          className={`p-1.5 rounded transition-colors ${
            isDefault
              ? "text-mid-gray/30 cursor-default"
              : "text-mid-gray hover:bg-mid-gray/20 cursor-pointer"
          }`}
          disabled={isDefault}
          title={t("settings.advanced.accentColor.reset")}
        >
          <RotateCcw className="w-4 h-4" />
        </button>
        <input
          type="text"
          value={hexInput}
          onChange={(e) => setHexInput(e.target.value)}
          onBlur={() => applyHex(hexInput)}
          onKeyDown={(e) => {
            if (e.key === "Enter") applyHex(hexInput);
          }}
          maxLength={7}
          className="w-[4.5rem] px-2 py-1 text-sm font-mono rounded border border-mid-gray/40 bg-mid-gray/10 focus:border-logo-primary focus:outline-none"
          placeholder="faa2ca"
        />
        <input
          type="color"
          value={currentColor}
          onChange={(e) => updateSetting("accent_color", e.target.value)}
          className="w-8 h-8 rounded-full cursor-pointer border border-mid-gray/40 bg-transparent p-0 [&::-webkit-color-swatch-wrapper]:p-0 [&::-webkit-color-swatch]:rounded-full [&::-webkit-color-swatch]:border-none [&::-moz-color-swatch]:rounded-full [&::-moz-color-swatch]:border-none"
        />
      </div>
    </SettingContainer>
  );
};
