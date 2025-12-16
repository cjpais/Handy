import React, { useState, useEffect } from "react";
import { Eye, EyeOff } from "lucide-react";
import { Input } from "../../ui/Input";

interface ApiKeyFieldProps {
  value: string;
  onBlur: (value: string) => void;
  disabled: boolean;
  placeholder?: string;
  className?: string;
  providerId?: string; // Used to reset local state when provider changes
}

export const ApiKeyField: React.FC<ApiKeyFieldProps> = React.memo(
  ({ value, onBlur, disabled, placeholder, className = "", providerId }) => {
    const [localValue, setLocalValue] = useState(value);
    const [showPassword, setShowPassword] = useState(false);

    // Reset local state when provider changes or value changes
    useEffect(() => {
      setLocalValue(value);
      setShowPassword(false); // Hide password when provider changes
    }, [value, providerId]);

    return (
      <div className={`relative flex items-center ${className}`}>
        <Input
          type={showPassword ? "text" : "password"}
          value={localValue}
          onChange={(event) => setLocalValue(event.target.value)}
          onBlur={() => onBlur(localValue)}
          placeholder={placeholder}
          variant="compact"
          disabled={disabled}
          className="flex-1 min-w-[320px] pr-10"
        />
        <button
          type="button"
          onClick={() => setShowPassword(!showPassword)}
          className="absolute right-2 p-1 text-text/50 hover:text-logo-primary transition-colors"
          title={showPassword ? "Hide API key" : "Show API key"}
        >
          {showPassword ? (
            <EyeOff className="w-4 h-4" />
          ) : (
            <Eye className="w-4 h-4" />
          )}
        </button>
      </div>
    );
  },
);

ApiKeyField.displayName = "ApiKeyField";
