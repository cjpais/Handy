import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type { SavedApiKey } from "@/bindings";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";

interface ApiKeyFieldProps {
  /** The provider's saved key, if it has one. The key itself never reaches the page. */
  saved: SavedApiKey | undefined;
  /** Saves a new key, or removes the saved one when given "". */
  onSave: (value: string) => void;
  disabled: boolean;
  placeholder?: string;
  className?: string;
}

/**
 * An empty field that saves what's typed into it. A saved key shows as the
 * placeholder, with a button to remove it.
 */
export const ApiKeyField: React.FC<ApiKeyFieldProps> = React.memo(
  ({ saved, onSave, disabled, placeholder, className = "" }) => {
    const { t } = useTranslation();
    const [localValue, setLocalValue] = useState("");

    const commit = () => {
      const trimmed = localValue.trim();
      setLocalValue("");
      if (trimmed) onSave(trimmed);
    };

    const savedPlaceholder = saved
      ? saved.hint
        ? t("settings.postProcessing.api.apiKey.savedWithHint", {
            hint: saved.hint,
          })
        : t("settings.postProcessing.api.apiKey.saved")
      : placeholder;

    return (
      <>
        <Input
          type="password"
          value={localValue}
          onChange={(event) => setLocalValue(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
          }}
          placeholder={savedPlaceholder}
          autoComplete="off"
          variant="compact"
          disabled={disabled}
          className={`flex-1 min-w-[320px] ${className}`}
        />
        {saved && (
          <Button
            variant="secondary"
            size="md"
            onClick={() => onSave("")}
            disabled={disabled}
          >
            {t("settings.postProcessing.api.apiKey.remove")}
          </Button>
        )}
      </>
    );
  },
);

ApiKeyField.displayName = "ApiKeyField";
