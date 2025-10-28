import React from "react";
import { Dropdown, type DropdownOption } from "../../ui/Dropdown";

interface ProviderSelectProps {
  options: DropdownOption[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

export const ProviderSelect: React.FC<ProviderSelectProps> = React.memo(
  ({ options, value, onChange, disabled }) => {
    return (
      <Dropdown
        options={options}
        selectedValue={value}
        onSelect={onChange}
        disabled={disabled}
        className="flex-1"
      />
    );
  },
);

ProviderSelect.displayName = "ProviderSelect";
