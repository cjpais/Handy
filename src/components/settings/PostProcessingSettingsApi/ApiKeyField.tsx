import React, { useEffect, useState } from "react";
import type { SavedApiKey } from "@/bindings";
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

/** What the field shows for a saved key: dots and the key's last characters. */
const maskFor = (saved: SavedApiKey) => `••••••••${saved.hint}`;

/**
 * A saved key shows as a mask. Typing over it saves a new key, clearing it
 * removes the key, and leaving it as it is changes nothing.
 */
export const ApiKeyField: React.FC<ApiKeyFieldProps> = React.memo(
  ({ saved, onSave, disabled, placeholder, className = "" }) => {
    const mask = saved ? maskFor(saved) : "";
    const [localValue, setLocalValue] = useState(mask);

    useEffect(() => {
      setLocalValue(mask);
    }, [mask]);

    const commit = () => {
      const trimmed = localValue.trim();
      if (trimmed === mask) return;
      if (trimmed === "" && !saved) return;
      onSave(trimmed);
    };

    const showingMask = saved !== undefined && localValue === mask;

    return (
      <Input
        // The mask is no secret, and showing its end tells keys apart.
        type={showingMask ? "text" : "password"}
        value={localValue}
        onChange={(event) => setLocalValue(event.target.value)}
        // Typing or pasting replaces the mask instead of adding to it.
        onFocus={(event) => event.currentTarget.select()}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        variant="compact"
        disabled={disabled}
        className={`flex-1 min-w-[320px] ${className}`}
      />
    );
  },
);

ApiKeyField.displayName = "ApiKeyField";
