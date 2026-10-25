import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import {
  keycapLabel,
  spaceDoesNotClick,
  type OSType,
} from "@/lib/utils/keyboard";
import { usePressAway } from "@/hooks/usePressAway";
import { languageList } from "@/lib/constants/languages";
import { CapturedKeys, Keycaps } from "../shared/Keycaps";
import { Switch } from "../ui/Switch";
import TrayIcon from "../icons/TrayIcon";

interface TrayPanelProps {
  os: OSType;
  languages: string[];
  modelName: string;
  /** The model is on disk. */
  ready: boolean;
  /** Shown while not ready. */
  download: React.ReactNode;
  shortcut: string;
  keysDown: boolean;
  capture: {
    capturing: boolean;
    partial: string;
    start: () => void;
    cancel: () => void;
  };
  atLogin: boolean;
  onAtLogin: (on: boolean) => void;
  onChangeLanguages: () => void;
  onChangeModel: () => void;
  /** Where the tail meets the icon, from the panel's inline-end edge. */
  tailAt: number;
  style?: React.CSSProperties;
}

const Chevron = () => (
  <svg
    viewBox="0 0 16 16"
    width="14"
    height="14"
    aria-hidden="true"
    className="try-panel-chevron"
  >
    <path
      d="M6 3.5 10.5 8 6 12.5"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

/**
 * All set's summary panel under the drawn tray icon. Clicks here never take
 * focus from the note, so the shortcut keeps typing there.
 */
export const TrayPanel: React.FC<TrayPanelProps> = ({
  os,
  languages,
  modelName,
  ready,
  download,
  shortcut,
  keysDown,
  capture,
  atLogin,
  onAtLogin,
  onChangeLanguages,
  onChangeModel,
  tailAt,
  style,
}) => {
  const { t, i18n } = useTranslation();
  const languagesText = languageList(languages, i18n.language);
  const keysLabel = keycapLabel(shortcut, os);

  // While capturing, a click outside the keys box cancels; that same click
  // must not then restart capture.
  const keysRow = useRef<HTMLDivElement>(null);
  const keysBox = useRef<HTMLSpanElement>(null);
  const resetAt = useRef(-Infinity);
  const { capturing } = capture;
  usePressAway(capturing, keysBox, (at) => {
    if (keysRow.current?.contains(at)) resetAt.current = performance.now();
    capture.cancel();
  });

  return (
    <div
      className="try-panel"
      style={{ ...style, ["--try-tail" as string]: `${tailAt}px` }}
      onMouseDown={(ev) => ev.preventDefault()}
    >
      <div className="try-panel-head">
        <TrayIcon state="idle" size={17} />
        <span className="try-panel-name">
          {t("onboarding.done.panel.name")}
        </span>
        {ready && (
          <span className="try-panel-ready">
            {t("onboarding.done.panel.ready")}
          </span>
        )}
      </div>
      {!ready && <div className="try-panel-download">{download}</div>}
      <button
        type="button"
        className="try-panel-row"
        aria-label={t("onboarding.done.panel.changeLanguages", {
          languages: languagesText,
        })}
        onClick={onChangeLanguages}
      >
        <span className="try-panel-label">
          {t("onboarding.done.panel.languages")}
        </span>
        <span className="try-panel-value">{languagesText}</span>
        <Chevron />
      </button>
      <button
        type="button"
        className="try-panel-row"
        aria-label={t("onboarding.done.panel.changeModel", {
          model: modelName,
        })}
        onClick={onChangeModel}
      >
        <span className="try-panel-label">
          {t("onboarding.done.panel.model")}
        </span>
        <span className="try-panel-value" title={modelName}>
          {modelName}
        </span>
        <Chevron />
      </button>
      <div
        ref={keysRow}
        className="try-panel-keys"
        data-capturing={capturing || undefined}
      >
        <button
          type="button"
          className="try-panel-row group/keycaps"
          aria-label={
            capturing
              ? t("onboarding.demo.pressKeys")
              : t("onboarding.try.changeKeys", { keys: keysLabel })
          }
          {...spaceDoesNotClick}
          onClick={() => {
            // The click that just cancelled capture.
            if (performance.now() - resetAt.current < 800) return;
            if (!capturing) capture.start();
          }}
        >
          <span className="try-panel-label">
            {t("onboarding.done.panel.shortcut")}
          </span>
          <span ref={keysBox} className="try-panel-keybox">
            {!capturing ? (
              <Keycaps
                binding={shortcut}
                osType={os}
                size="line"
                pressed={keysDown}
                lift
              />
            ) : (
              <CapturedKeys
                partial={capture.partial}
                osType={os}
                size="line"
                slotLabel={t("settings.general.shortcut.pressKeys")}
              />
            )}
          </span>
        </button>
      </div>
      <hr className="try-panel-rule" />
      <div className="try-panel-row is-switch">
        {/* Switch draws its own <label>; the words toggle it too. */}
        <span
          id="try-panel-login"
          className="try-panel-label"
          onClick={() => onAtLogin(!atLogin)}
        >
          {t("onboarding.done.atLogin")}
        </span>
        <Switch
          checked={atLogin}
          onChange={onAtLogin}
          ariaLabelledBy="try-panel-login"
        />
      </div>
    </div>
  );
};
