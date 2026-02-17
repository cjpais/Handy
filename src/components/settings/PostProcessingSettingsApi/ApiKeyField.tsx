import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pencil, X } from "lucide-react";
import { Input } from "../../ui/Input";
import { Button } from "../../ui/Button";

interface ApiKeyFieldProps {
  hint: string | null;
  onSave: (value: string) => void;
  onClear: () => void;
  disabled: boolean;
  placeholder?: string;
  className?: string;
}

export const ApiKeyField: React.FC<ApiKeyFieldProps> = React.memo(
  ({ hint, onSave, onClear, disabled, placeholder, className = "" }) => {
    const { t } = useTranslation();
    const [isEditing, setIsEditing] = useState(false);
    const [localValue, setLocalValue] = useState("");

    const handleStartEdit = () => {
      setLocalValue("");
      setIsEditing(true);
    };

    const handleSave = () => {
      const trimmed = localValue.trim();
      if (trimmed) {
        onSave(trimmed);
      }
      setLocalValue("");
      setIsEditing(false);
    };

    const handleBlur = () => {
      const trimmed = localValue.trim();
      if (trimmed) {
        onSave(trimmed);
      }
      setLocalValue("");
      setIsEditing(false);
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        handleSave();
      } else if (e.key === "Escape") {
        setLocalValue("");
        setIsEditing(false);
      }
    };

    // No key saved — show input directly
    if (!hint && !isEditing) {
      return (
        <Input
          type="password"
          value={localValue}
          onChange={(event) => setLocalValue(event.target.value)}
          onBlur={() => {
            const trimmed = localValue.trim();
            if (trimmed) {
              onSave(trimmed);
              setLocalValue("");
            }
          }}
          onKeyDown={handleKeyDown}
          onFocus={() => setIsEditing(true)}
          placeholder={placeholder}
          variant="compact"
          disabled={disabled}
          className={`flex-1 min-w-[320px] ${className}`}
        />
      );
    }

    // Key saved and not editing — show masked hint
    if (hint && !isEditing) {
      return (
        <div
          className={`flex items-center gap-2 flex-1 min-w-[320px] ${className}`}
        >
          <span className="font-mono text-sm text-text/70 select-none">
            {hint}
          </span>
          <Button
            variant="ghost"
            size="sm"
            onClick={handleStartEdit}
            disabled={disabled}
            aria-label={t("settings.postProcessing.api.apiKey.edit")}
          >
            <Pencil className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={onClear}
            disabled={disabled}
            aria-label={t("settings.postProcessing.api.apiKey.clear")}
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      );
    }

    // Editing mode
    return (
      <Input
        type="password"
        value={localValue}
        onChange={(event) => setLocalValue(event.target.value)}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        variant="compact"
        disabled={disabled}
        autoFocus
        className={`flex-1 min-w-[320px] ${className}`}
      />
    );
  },
);

ApiKeyField.displayName = "ApiKeyField";
