import React, { useCallback, useRef, useState } from "react";
import Badge from "./Badge";

export type TagInputAddRejection =
  | "empty"
  | "duplicate"
  | "tooLong"
  | "tooMany";

interface TagInputProps {
  value: readonly string[];
  onChange: (next: string[]) => void;
  onAddRejected?: (reason: TagInputAddRejection) => void;
  placeholder?: string;
  disabled?: boolean;
  removeAriaLabel?: string;
  maxTermLength?: number;
  maxTerms?: number;
  className?: string;
  inputId?: string;
  inputAriaLabel?: string;
}

export const TagInput: React.FC<TagInputProps> = ({
  value,
  onChange,
  onAddRejected,
  placeholder,
  disabled,
  removeAriaLabel = "Remove",
  maxTermLength,
  maxTerms,
  className = "",
  inputId,
  inputAriaLabel,
}) => {
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const tryAdd = useCallback(
    (raw: string): boolean => {
      const trimmed = raw.trim();
      if (trimmed === "") {
        onAddRejected?.("empty");
        return false;
      }
      if (
        typeof maxTermLength === "number" &&
        [...trimmed].length > maxTermLength
      ) {
        onAddRejected?.("tooLong");
        return false;
      }
      if (value.includes(trimmed)) {
        onAddRejected?.("duplicate");
        return false;
      }
      if (typeof maxTerms === "number" && value.length >= maxTerms) {
        onAddRejected?.("tooMany");
        return false;
      }
      onChange([...value, trimmed]);
      return true;
    },
    [maxTermLength, maxTerms, onAddRejected, onChange, value],
  );

  const removeAt = useCallback(
    (index: number) => {
      const next = value.slice(0, index).concat(value.slice(index + 1));
      onChange(next);
    },
    [onChange, value],
  );

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      if (tryAdd(draft)) {
        setDraft("");
      }
      return;
    }

    if (event.key === "Backspace" && draft === "" && value.length > 0) {
      event.preventDefault();
      removeAt(value.length - 1);
    }
  };

  const handleBlur = () => {
    if (draft.trim() !== "" && tryAdd(draft)) {
      setDraft("");
    }
  };

  const containerClasses = disabled
    ? "cursor-not-allowed border-slate-200 bg-slate-100 opacity-60"
    : "border-slate-300 bg-white hover:border-slate-400 focus-within:border-red-400 focus-within:ring-4 focus-within:ring-red-100";

  return (
    <div
      className={`flex min-h-[2.5rem] flex-wrap items-center gap-1.5 rounded-xl border px-2 py-1.5 text-sm transition-all duration-150 ${containerClasses} ${className}`}
      onClick={() => inputRef.current?.focus()}
    >
      {value.map((tag, index) => (
        <Badge key={`${tag}-${index}`} variant="secondary" className="gap-1">
          <span className="break-all">{tag}</span>
          <button
            type="button"
            disabled={disabled}
            onClick={(event) => {
              event.stopPropagation();
              removeAt(index);
            }}
            className="-mr-1 inline-flex h-4 w-4 items-center justify-center rounded-full text-text/70 transition-colors hover:bg-mid-gray/30 hover:text-text disabled:cursor-not-allowed disabled:opacity-50"
            aria-label={`${removeAriaLabel}: ${tag}`}
          >
            <span aria-hidden>×</span>
          </button>
        </Badge>
      ))}
      <input
        ref={inputRef}
        id={inputId}
        aria-label={inputAriaLabel}
        type="text"
        value={draft}
        disabled={disabled}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={handleKeyDown}
        onBlur={handleBlur}
        placeholder={value.length === 0 ? placeholder : undefined}
        className="min-w-[8rem] flex-1 bg-transparent px-1 py-0.5 text-sm text-slate-900 placeholder:text-slate-400 focus:outline-none disabled:cursor-not-allowed"
      />
    </div>
  );
};
