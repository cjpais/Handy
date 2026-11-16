import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { InfoTooltip } from "../../ui/InfoTooltip";
import { Input } from "../../ui/Input";
import { Textarea } from "../../ui/Textarea";

/** Where this section's strings live. */
export const K = "settings.advanced.remoteTranscription";

/**
 * A preset's name in the user's language. Descriptive names are translated;
 * product names (OpenThai2.0 ASR, MLX Audio) are the same everywhere.
 */
export const usePresetName = () => {
  const { t, i18n } = useTranslation();
  return (key: string, fallback: string) => {
    const translated = `${K}.presets.names.${key}`;
    return i18n.exists(translated) ? t(translated) : fallback;
  };
};

/** The settings a `placeholder` error can name, by the backend's word. */
const SETTING_LABELS = {
  URL: `${K}.url.label`,
  Prompt: `${K}.prompt.label`,
  JSON: `${K}.jsonBody.label`,
} as const;

/**
 * A backend error as text. Settings errors come as `code|detail` and are
 * translated; anything else (what a server said, a network failure) is shown
 * as it is.
 */
export const useErrorText = () => {
  const { t, i18n } = useTranslation();
  return (raw: string) => {
    const [code, ...rest] = raw.split("|");
    const key = `${K}.errors.${code}`;
    if (!i18n.exists(key)) return raw;
    let detail = rest.join("|");
    // The backend names the setting; show it as the settings page does.
    if (code === "placeholder" && detail in SETTING_LABELS)
      detail = t(SETTING_LABELS[detail as keyof typeof SETTING_LABELS]);
    return t(key, { detail });
  };
};

/** A text input that reports its value when it loses focus. */
export const BlurInput: React.FC<
  Omit<React.InputHTMLAttributes<HTMLInputElement>, "onChange" | "onBlur"> & {
    value: string;
    onCommit: (value: string) => void;
  }
> = ({ value, onCommit, className = "", ...props }) => {
  const [local, setLocal] = useState(value);
  useEffect(() => setLocal(value), [value]);
  return (
    <Input
      {...props}
      value={local}
      onChange={(e) => setLocal(e.target.value)}
      onBlur={() => local !== value && onCommit(local)}
      variant="compact"
      autoComplete="off"
      spellCheck={false}
      className={`w-full min-w-0 ${className}`}
    />
  );
};

/** A multi-line text setting (a prompt, a JSON body), saved when it loses focus. */
export const LongTextInput: React.FC<{
  label: string;
  value: string;
  rows: number;
  mono?: boolean;
  onCommit: (value: string) => void;
}> = ({ label, value, rows, mono = false, onCommit }) => {
  const [local, setLocal] = useState(value);
  useEffect(() => setLocal(value), [value]);
  return (
    <Textarea
      value={local}
      onChange={(e) => setLocal(e.target.value)}
      onBlur={() => local !== value && onCommit(local)}
      aria-label={label}
      spellCheck={false}
      rows={rows}
      variant="compact"
      className={`w-full !font-normal ${mono ? "font-mono text-xs" : "text-sm"}`}
    />
  );
};

/** One labelled row; every row shares the label column so inputs line up. */
export const Field: React.FC<{
  title: string;
  description: string;
  children: React.ReactNode;
}> = ({ title, description, children }) => (
  <div className="grid grid-cols-[132px_minmax(0,1fr)] items-start gap-3 px-4 py-2">
    <div className="flex min-h-[30px] items-center gap-1.5">
      <h3 className="text-sm font-medium">{title}</h3>
      <InfoTooltip text={description} />
    </div>
    <div className="min-w-0">{children}</div>
  </div>
);
