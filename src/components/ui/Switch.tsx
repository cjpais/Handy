import React from "react";

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  /** Names the switch for assistive tech when no visible label points at it. */
  ariaLabel?: string;
  /** The id of a visible label that names the switch. */
  ariaLabelledBy?: string;
}

/** The on/off switch on its own; ToggleSwitch puts it in a settings row. */
export const Switch: React.FC<SwitchProps> = ({
  checked,
  onChange,
  disabled = false,
  ariaLabel,
  ariaLabelledBy,
}) => {
  return (
    <label
      className={`flex items-center ${disabled ? "cursor-not-allowed" : "cursor-pointer"}`}
    >
      <input
        type="checkbox"
        aria-label={ariaLabel}
        aria-labelledby={ariaLabelledBy}
        value=""
        className="sr-only peer"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <div className="relative w-11 h-6 bg-line peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-logo-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-line-strong after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-background-ui peer-disabled:opacity-50"></div>
    </label>
  );
};
