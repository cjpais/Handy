import React from "react";
import SelectComponent from "react-select";
import CreatableSelect from "react-select/creatable";
import type {
  ActionMeta,
  Props as ReactSelectProps,
  SingleValue,
  StylesConfig,
} from "react-select";

export type SelectOption = {
  value: string;
  label: string;
  isDisabled?: boolean;
};

type BaseProps = {
  value: string | null;
  options: SelectOption[];
  placeholder?: string;
  disabled?: boolean;
  isLoading?: boolean;
  isClearable?: boolean;
  onChange: (value: string | null, action: ActionMeta<SelectOption>) => void;
  onBlur?: () => void;
  className?: string;
  formatCreateLabel?: (input: string) => string;
};

type CreatableProps = {
  isCreatable: true;
  onCreateOption: (value: string) => void;
};

type NonCreatableProps = {
  isCreatable?: false;
  onCreateOption?: never;
};

export type SelectProps = BaseProps & (CreatableProps | NonCreatableProps);

const selectStyles: StylesConfig<SelectOption, false> = {
  control: (base, state) => ({
    ...base,
    minHeight: 42,
    borderRadius: 12, // matching --radius-inputs
    borderColor: state.isFocused
      ? "var(--color-forest-green)"
      : "var(--color-stone-mist)",
    boxShadow: state.isFocused ? "0 0 0 3px rgba(29, 122, 70, 0.15)" : "none",
    backgroundColor: "var(--color-orange-off-white)",
    fontSize: "1rem", // text-base
    color: "var(--color-charcoal)",
    transition: "all 150ms ease",
    ":hover": {
      borderColor: state.isFocused
        ? "var(--color-forest-green)"
        : "var(--color-bark-grey)",
    },
  }),
  valueContainer: (base) => ({
    ...base,
    paddingInline: 16,
    paddingBlock: 8,
  }),
  input: (base) => ({
    ...base,
    color: "var(--color-charcoal)",
    margin: 0,
    padding: 0,
  }),
  singleValue: (base) => ({
    ...base,
    color: "var(--color-charcoal)",
  }),
  dropdownIndicator: (base, state) => ({
    ...base,
    color: state.isFocused
      ? "var(--color-forest-green)"
      : "var(--color-bark-grey)",
    ":hover": {
      color: "var(--color-forest-green)",
    },
  }),
  clearIndicator: (base) => ({
    ...base,
    color: "var(--color-bark-grey)",
    ":hover": {
      color: "var(--color-alarm-red)",
    },
  }),
  menu: (provided) => ({
    ...provided,
    zIndex: 30,
    backgroundColor: "var(--color-orange-off-white)",
    color: "var(--color-charcoal)",
    border: "1px solid var(--color-stone-mist)",
    borderRadius: 12,
    boxShadow: "var(--shadow-xl)",
  }),
  option: (base, state) => ({
    ...base,
    backgroundColor: state.isSelected
      ? "var(--color-forest-green)"
      : state.isFocused
        ? "color-mix(in srgb, var(--color-forest-green) 10%, transparent)"
        : "transparent",
    color: state.isSelected
      ? "var(--color-orange-off-white)"
      : "var(--color-charcoal)",
    cursor: state.isDisabled ? "not-allowed" : base.cursor,
    opacity: state.isDisabled ? 0.5 : 1,
    paddingInline: 16,
    paddingBlock: 10,
    ":active": {
      backgroundColor: state.isSelected
        ? "var(--color-deep-forest-green)"
        : "color-mix(in srgb, var(--color-forest-green) 20%, transparent)",
    },
  }),
  placeholder: (base) => ({
    ...base,
    color: "var(--color-pebble)",
  }),
};

export const Select: React.FC<SelectProps> = React.memo(
  ({
    value,
    options,
    placeholder,
    disabled,
    isLoading,
    isClearable = true,
    onChange,
    onBlur,
    className = "",
    isCreatable,
    formatCreateLabel,
    onCreateOption,
  }) => {
    const selectValue = React.useMemo(() => {
      if (!value) return null;
      const existing = options.find((option) => option.value === value);
      if (existing) return existing;
      return { value, label: value, isDisabled: false };
    }, [value, options]);

    const handleChange = (
      option: SingleValue<SelectOption>,
      action: ActionMeta<SelectOption>,
    ) => {
      onChange(option?.value ?? null, action);
    };

    const sharedProps: Partial<ReactSelectProps<SelectOption, false>> = {
      className,
      classNamePrefix: "app-select",
      value: selectValue,
      options,
      onChange: handleChange,
      placeholder,
      isDisabled: disabled,
      isLoading,
      onBlur,
      isClearable,
      styles: selectStyles,
    };

    if (isCreatable) {
      return (
        <CreatableSelect<SelectOption, false>
          {...sharedProps}
          onCreateOption={onCreateOption}
          formatCreateLabel={formatCreateLabel}
        />
      );
    }

    return <SelectComponent<SelectOption, false> {...sharedProps} />;
  },
);

Select.displayName = "Select";
