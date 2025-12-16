import React, { useState, useEffect } from "react";
import type { ModelOption } from "./types";
import { Select } from "../../ui/Select";

type ModelSelectProps = {
  value: string;
  options: ModelOption[];
  disabled?: boolean;
  placeholder?: string;
  isLoading?: boolean;
  onSelect: (value: string) => void;
  onCreate: (value: string) => void;
  onBlur: () => void;
  className?: string;
  providerId?: string; // Track provider changes to reset menu state
};

export const ModelSelect: React.FC<ModelSelectProps> = React.memo(
  ({
    value,
    options,
    disabled,
    placeholder,
    isLoading,
    onSelect,
    onCreate,
    onBlur,
    className = "flex-1 min-w-[360px]",
    providerId,
  }) => {
    // Track if menu should be open - starts open if no value selected
    const [isMenuOpen, setIsMenuOpen] = useState(!value);

    // Reset menu state when provider changes - open if no model for new provider
    useEffect(() => {
      if (!value) {
        setIsMenuOpen(true);
      }
    }, [providerId, value]);

    const handleCreate = (inputValue: string) => {
      const trimmed = inputValue.trim();
      if (!trimmed) return;
      onCreate(trimmed);
      setIsMenuOpen(false);
    };

    const handleSelect = (selected: string | null) => {
      onSelect(selected ?? "");
      setIsMenuOpen(false);
    };

    const computedClassName = `text-sm ${className}`;

    return (
      <Select
        className={computedClassName}
        value={value || null}
        options={options}
        onChange={handleSelect}
        onCreateOption={handleCreate}
        onBlur={() => {
          setIsMenuOpen(false);
          onBlur();
        }}
        placeholder={placeholder}
        disabled={disabled}
        isLoading={isLoading}
        isCreatable
        formatCreateLabel={(input) => `Use "${input}"`}
        menuIsOpen={isMenuOpen}
      />
    );
  },
);

ModelSelect.displayName = "ModelSelect";
