import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { keycapParts, type OSType } from "@/lib/utils/keyboard";
import { languageName } from "@/lib/constants/languages";
import { Keycaps } from "../shared/Keycaps";
import { Switch } from "../ui/Switch";
import TrayIcon from "../icons/TrayIcon";

interface TrayPanelProps {
  os: OSType;
  /** Spoken languages (codes), as picked on Welcome. */
  languages: string[];
  modelName: string;
  /** The model's on disk: Handy's ready. */
  ready: boolean;
  /** While it isn't: the download, as Demo shows it. */
  download: React.ReactNode;
  shortcut: string;
  keysDown: boolean;
  /** Try it's recorder for new keys. */
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
 * All set's panel, hanging off Handy's icon in the drawn menu bar or tray, in
 * the bubble's material (the glove's speech, come back as its menu): what
 * they set up. Languages and the model lead back to their screens, which
 * come straight back here; the shortcut and login change in place. Clicks
 * here never take the cursor out of the note, so the shortcut keeps typing
 * there.
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
  const ui = i18n.language;
  const languageList = new Intl.ListFormat(ui, { type: "conjunction" }).format(
    languages.map((code) => languageName(code, ui)),
  );
  const keysLabel = keycapParts(shortcut, os)
    .map((part) => part.label)
    .join(os === "macos" ? " " : " + ");
  const partialKeys = capture.partial
    ? capture.partial.split("+").filter(Boolean)
    : [];

  // New keys going in: a click anywhere but the keys' box puts the old ones
  // back, the row's own words too. That click mustn't then start again.
  const keysRow = useRef<HTMLDivElement>(null);
  const keysBox = useRef<HTMLSpanElement>(null);
  /** When a click on the row's words reset it, so that click doesn't start it again. */
  const resetAt = useRef(-Infinity);
  const { capturing, cancel } = capture;
  useEffect(() => {
    if (!capturing) return;
    const away = (ev: PointerEvent) => {
      const at = ev.target as Node;
      if (keysBox.current?.contains(at)) return;
      if (keysRow.current?.contains(at)) resetAt.current = performance.now();
      cancel();
    };
    document.addEventListener("pointerdown", away, true);
    return () => document.removeEventListener("pointerdown", away, true);
  }, [capturing, cancel]);

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
          languages: languageList,
        })}
        onClick={onChangeLanguages}
      >
        <span className="try-panel-label">
          {t("onboarding.done.panel.languages")}
        </span>
        <span className="try-panel-value">{languageList}</span>
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
      {/* As in Settings: a click on the row turns the keys into "Press
          keys…", then what's held; a click anywhere but that box puts the
          old ones back. One row throughout, the keys' place at its end, so
          nothing moves. */}
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
          // The shortcut may use Space; pressing it here mustn't click.
          onKeyDown={(ev) => ev.key === " " && ev.preventDefault()}
          onKeyUp={(ev) => ev.key === " " && ev.preventDefault()}
          onClick={() => {
            // The same click that reset it (a press and release, well
            // under a second): leave it reset.
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
            ) : partialKeys.length ? (
              <Keycaps
                binding={capture.partial}
                osType={os}
                size="line"
                partial
              />
            ) : (
              <span className="try-panel-slot" role="status">
                {t("settings.general.shortcut.pressKeys")}
              </span>
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
