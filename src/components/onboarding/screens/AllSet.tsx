import React, { useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutPress } from "@/hooks/useShortcutPress";
import type { OSType } from "@/lib/utils/keyboard";
import { Button } from "../../ui/Button";
import { Switch } from "../../ui/Switch";
import { Keycaps } from "../../shared/Keycaps";
import { Mascot } from "../../shared/Mascot";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { useSuspendedShortcuts } from "../useSuspendedShortcuts";

const LINES = ["highFive", "upTop", "again", "reallySet"] as const;

interface AllSetProps {
  progress: { index: number; count: number };
  os: OSType;
  onBack: () => void;
  onFinish: () => void;
}

/**
 * Screen 6: you're done, Handy keeps running in the menu bar or tray, and it
 * can start at login. The glove waves; the shortcut (or a click) high-fives
 * it. The global shortcut is suspended here so that press doesn't record.
 */
export const AllSet: React.FC<AllSetProps> = ({
  progress,
  os,
  onBack,
  onFinish,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const { preview } = useOnboardingStore();
  const shortcut = getSetting("bindings")?.transcribe?.current_binding ?? "";
  const threshold = getSetting("hold_threshold_ms") ?? 300;
  const [fives, setFives] = useState(0);
  const [down, setDown] = useState(false);
  // On by default; the choice is applied on Finish so toggling here doesn't
  // register and unregister the login item (each one posts a macOS notice).
  const [launchAtLogin, setLaunchAtLogin] = useState(true);

  const finish = async () => {
    if (!preview) await updateSetting("autostart_enabled", launchAtLogin);
    onFinish();
  };

  useSuspendedShortcuts(!preview);

  const slap = () => setFives((n) => n + 1);
  useShortcutPress(shortcut, os, threshold, {
    onDown: () => {
      setDown(true);
      slap();
    },
    onUp: () => setDown(false),
  });

  const title = t(
    `onboarding.done.${LINES[Math.min(fives, LINES.length - 1)]}`,
  );

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      left={
        <label className="inline-flex cursor-pointer items-center gap-[10px] text-small text-ink-2">
          <Switch
            checked={launchAtLogin}
            ariaLabelledBy="onboarding-login-label"
            onChange={setLaunchAtLogin}
          />
          <span id="onboarding-login-label">
            {t("onboarding.done.atLogin")}
          </span>
        </label>
      }
      primary={
        <Button variant="primary" size="lg" onClick={finish}>
          {t("onboarding.done.finish")}
        </Button>
      }
    >
      <ScreenMain>
        <button
          type="button"
          aria-label={t("onboarding.done.highFiveLabel")}
          onClick={slap}
          className="relative mb-[14px] cursor-pointer rounded-[28px] border-0 bg-transparent p-[6px]"
        >
          <Mascot key={fives} size={168} animation={fives ? "slap" : "wave"} />
          {fives > 0 && (
            <span
              key={`burst-${fives}`}
              aria-hidden="true"
              className="pointer-events-none absolute start-1/2 -top-[30px] -ms-[30px] animate-burst opacity-0"
            >
              <svg
                viewBox="0 0 60 60"
                width="60"
                height="60"
                className="block fill-none stroke-background-ui"
                strokeWidth="4"
                strokeLinecap="round"
              >
                <path d="M30 22V6M40 26l10-10M20 26 10 16" />
              </svg>
            </span>
          )}
        </button>
        {/* Tall lines leave room for the keycaps in the sentence. */}
        <ScreenHeader
          hero
          title={title}
          subClassName="mx-auto max-w-[440px] leading-[2]"
          sub={
            <Trans
              i18nKey={
                os === "macos"
                  ? "onboarding.done.bodyMac"
                  : "onboarding.done.bodyTray"
              }
              components={{
                keys: (
                  <Keycaps
                    binding={shortcut}
                    osType={os}
                    pressed={down}
                    className="mx-[2px] align-middle"
                  />
                ),
              }}
            />
          }
        />
      </ScreenMain>
    </OnboardingShell>
  );
};
