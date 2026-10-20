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
      {/* v2: 44 × 24 with a 20px knob in px (the rem sizes above land at
          41 × 22.5 on the 15px root), an ink 20% track, a shadowed knob */}
      <div className="relative w-11 h-6 bg-mid-gray/20 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-logo-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-background-ui peer-disabled:opacity-50 v2:w-[44px] v2:h-[24px] v2:bg-line-strong v2:peer-checked:bg-background-ui v2:peer-focus:ring-0 v2:peer-focus-visible:outline-2 v2:peer-focus-visible:outline-offset-2 v2:peer-focus-visible:outline-background-ui v2:after:h-[20px] v2:after:w-[20px] v2:after:border-0 v2:after:shadow-[0_1px_2px_rgba(0,0,0,0.25)]"></div>
    </label>
  );
};
