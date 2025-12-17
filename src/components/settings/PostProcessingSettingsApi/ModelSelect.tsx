import React, { useState, useEffect } from "react";
import { RefreshCcw } from "lucide-react";
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
  onRefresh?: () => void;
  isRefreshing?: boolean;
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
    onRefresh,
    isRefreshing,
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

    const handleRefreshClick = (e: React.MouseEvent) => {
      e.stopPropagation();
      e.preventDefault();
      if (onRefresh && !isRefreshing && !disabled) {
        onRefresh();
      }
    };

    const computedClassName = `text-sm ${className}`;

    // Custom dropdown indicator with refresh icon
    const customComponents = {
      DropdownIndicator: () => (
        <button
          type="button"
          onClick={handleRefreshClick}
          disabled={isRefreshing || disabled}
          className="p-2 mr-1 text-text/50 hover:text-logo-primary transition-colors disabled:opacity-50"
          title="Refresh models"
        >
          <RefreshCcw
            className={`h-4 w-4 ${isRefreshing ? "animate-spin" : ""}`}
          />
        </button>
      ),
    };

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
        components={customComponents}
      />
    );
  },
);

ModelSelect.displayName = "ModelSelect";

