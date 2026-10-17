import React from "react";
import "./glass-pill.css";
import { BAR_COUNT, barHeightPx, barLevels, formatElapsed } from "./levels";

export type GlassState =
  | "arming"
  | "recording"
  | "transcribing"
  | "done"
  | "error";

export interface GlassPillLabels {
  recording: string;
  arming: string;
  transcribing: string;
  done: string;
  cancel: string;
}

export interface GlassPillProps {
  state: GlassState;
  /** Smoothed microphone buckets, 0..1. */
  levels: number[];
  elapsedSeconds: number;
  /** Short message of the error state. */
  errorText?: string;
  labels: GlassPillLabels;
  /** Played when the capsule is dissolving. */
  leaving?: boolean;
  /**
   * A native blur sits behind the window (macOS vibrancy). Without it the
   * capsule gets a denser fill so it stays readable.
   */
  nativeGlass?: boolean;
  onCancel?: () => void;
}

const CheckIcon = () => (
  <svg className="vg-check" viewBox="0 0 24 24" aria-hidden="true">
    <circle className="vg-check-disc" cx="12" cy="12" r="10.5" />
    <path
      className="vg-check-mark"
      d="M7.2 12.6 10.6 16 17 8.6"
      fill="none"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      pathLength={1}
    />
  </svg>
);

const AlertIcon = () => (
  <svg className="vg-alert" viewBox="0 0 24 24" aria-hidden="true">
    <circle className="vg-alert-disc" cx="12" cy="12" r="10.5" />
    <path
      d="M12 7.2v6"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
    />
    <circle cx="12" cy="16.9" r="1.35" fill="currentColor" />
  </svg>
);

/**
 * The recording capsule: one pill that morphs between recording, recognizing,
 * done and error. Presentational only; the overlay window and the preview page
 * both feed it.
 */
export const GlassPill: React.FC<GlassPillProps> = ({
  state,
  levels,
  elapsedSeconds,
  errorText,
  labels,
  leaving = false,
  nativeGlass = true,
  onCancel,
}) => {
  const heights = barLevels(levels).map((level) => barHeightPx(level));
  const statusLabel =
    state === "arming"
      ? labels.arming
      : state === "recording"
        ? labels.recording
        : state === "transcribing"
          ? labels.transcribing
          : state === "done"
            ? labels.done
            : (errorText ?? "");
  const showWave = state === "arming" || state === "recording";

  return (
    <div
      className={`vg-pill is-${state} ${nativeGlass ? "has-native-glass" : "no-native-glass"} ${
        leaving ? "is-leaving" : ""
      }`}
      role="status"
      aria-live="polite"
      aria-label={statusLabel}
    >
      <span className="vg-pill-edge" aria-hidden="true" />

      <div className="vg-wave" aria-hidden="true">
        {Array.from({ length: BAR_COUNT }, (_, i) => (
          <i
            key={i}
            style={{ height: showWave ? `${heights[i]}px` : undefined }}
          />
        ))}
      </div>

      <div className="vg-flow" aria-hidden="true" />

      <span className="vg-timer" aria-hidden="true">
        {formatElapsed(elapsedSeconds)}
      </span>

      <span className="vg-label" aria-hidden="true">
        {state === "transcribing" ? labels.transcribing : ""}
      </span>

      <div className="vg-outcome" aria-hidden="true">
        {state === "done" && <CheckIcon />}
        {state === "error" && (
          <>
            <AlertIcon />
            <span className="vg-error-text">{errorText}</span>
          </>
        )}
      </div>

      <button
        type="button"
        className="vg-x"
        aria-label={labels.cancel}
        onClick={onCancel}
        tabIndex={state === "done" || state === "error" ? -1 : 0}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M4.5 4.5 11.5 11.5 M11.5 4.5 4.5 11.5"
            stroke="currentColor"
            strokeWidth="1.7"
            strokeLinecap="round"
          />
        </svg>
      </button>
    </div>
  );
};

export default GlassPill;
