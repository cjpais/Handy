import React, { useState } from "react";
import { Input } from "../../ui/Input";

interface AdditionalUrlFieldProps {
  value: string;
  onBlur: (value: string) => void;
  disabled: boolean;
  placeholder?: string;
  className?: string;
}

export const AdditionalUrlField: React.FC<AdditionalUrlFieldProps> = React.memo(
  ({ value, onBlur, disabled, placeholder, className = "" }) => {
    const [localValue, setLocalValue] = useState(value);

    React.useEffect(() => {
      setLocalValue(value);
    }, [value]);

    return (
      <Input
        type="text"
        value={localValue}
        onChange={(event) => setLocalValue(event.target.value)}
        onBlur={() => onBlur(localValue)}
        placeholder={placeholder}
        variant="compact"
        disabled={disabled}
        className={`flex-1 min-w-[360px] ${className}`}
      />
    );
  },
);

AdditionalUrlField.displayName = "AdditionalUrlField";
