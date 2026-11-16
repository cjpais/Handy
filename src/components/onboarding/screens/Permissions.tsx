import React, { useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { relaunch } from "@tauri-apps/plugin-process";
import { Keyboard, Mic } from "lucide-react";
import { Button } from "../../ui/Button";
import { Spinner } from "../../ui/Spinner";
import { TextLink } from "../../ui/TextLink";
import { DoneMark } from "../../shared/DoneMark";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import type { Permissions as PermissionsState } from "../usePermissions";

interface PermissionRowProps {
  icon: React.ReactNode;
  title: string;
  why: string;
  status: PermissionsState["microphone"];
  /** Its turn: the rows are allowed in order, so only this one is live. */
  next: boolean;
  action: string;
  /** While waiting, what to do in the OS, in place of `why`. */
  instruction?: string;
  onRequest: () => void;
}

/** One permission row; the status column's min width stops text re-wrapping. */
const PermissionRow: React.FC<PermissionRowProps> = ({
  icon,
  title,
  why,
  status,
  next,
  action,
  instruction,
  onRequest,
}) => {
  const { t } = useTranslation();
  const waiting = status === "waiting";
  return (
    <div className="grid grid-cols-[46px_1fr_minmax(112px,auto)] items-center gap-[18px] py-[18px] [&+&]:border-t [&+&]:border-line">
      {/* A label, not a key: neutral, and "Allowed" alone shows the grant. */}
      <span className="grid size-[46px] place-items-center rounded-[11px] bg-wash text-ink-2">
        {icon}
      </span>
      <div>
        <b className="block font-display text-row font-[650] leading-[1.3]">
          {title}
        </b>
        <p
          className={`mt-[2px] mb-0 text-small leading-[1.45] text-pretty ${
            waiting && instruction ? "text-text" : "text-ink-2"
          }`}
        >
          {(waiting && instruction) || why}
        </p>
      </div>
      {status === "granted" ? (
        <span className="inline-flex items-center justify-self-end gap-[6px] text-small font-semibold text-moss">
          <DoneMark />
          {t("onboarding.permissions.allowed")}
        </span>
      ) : waiting ? (
        <span className="inline-flex items-center justify-self-end gap-[6px] text-small text-ink-3">
          <Spinner className="shrink-0" />
          {t("onboarding.permissions.waiting")}
        </span>
      ) : (
        <Button
          variant="primary"
          size="md"
          disabled={!next}
          onClick={onRequest}
          className="justify-self-end"
        >
          {action}
        </Button>
      )}
    </div>
  );
};

interface PermissionsProps {
  permissions: PermissionsState;
  /** In the flow: its place and Back. Standalone (a returning user): neither. */
  progress?: { index: number; count: number };
  onBack?: () => void;
  onContinue: () => void;
}

/** Screen 2: microphone, plus Accessibility on macOS, granted in order. */
export const Permissions: React.FC<PermissionsProps> = ({
  permissions,
  progress,
  onBack,
  onContinue,
}) => {
  const { t } = useTranslation();
  const { welcomeBack, preview, set } = useOnboardingStore();
  const isMac = permissions.platform === "macos";
  const rows = isMac
    ? (["microphone", "accessibility"] as const)
    : (["microphone"] as const);
  const next = rows.find((r) => permissions[r] !== "granted");
  // Asking for the microphone opens the OS settings on Windows, and on macOS
  // once it has asked: then the row says what to turn on there.
  const [micInSettings, setMicInSettings] = useState(false);
  const turnOn = t(
    isMac
      ? "onboarding.permissions.turnOnMac"
      : "onboarding.permissions.turnOnWindows",
  );

  const relaunchHandy = async () => {
    if (preview) return;
    set({ step: "permissions", welcomeBack: true });
    await relaunch();
  };

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={
        permissions.allGranted
          ? ""
          : t(
              isMac
                ? "onboarding.permissions.hintBoth"
                : "onboarding.permissions.hintMicrophone",
            )
      }
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={!permissions.allGranted}
          onClick={() => {
            if (welcomeBack) set({ welcomeBack: false });
            onContinue();
          }}
        >
          {t("onboarding.shell.continue")}
        </Button>
      }
    >
      <ScreenMain
        header={
          <ScreenHeader
            above={
              welcomeBack && (
                <div className="flex animate-rise items-center justify-center gap-[10px] text-small font-semibold text-moss">
                  <DoneMark />
                  {t("onboarding.permissions.welcomeBack")}
                </div>
              )
            }
            title={t(
              isMac
                ? "onboarding.permissions.titleMac"
                : "onboarding.permissions.titleWindows",
            )}
            sub={t(
              isMac
                ? "onboarding.permissions.subtitleMac"
                : "onboarding.permissions.subtitleWindows",
            )}
          />
        }
      >
        <div className="flex w-full flex-col rounded-panel border border-line-strong px-[22px] py-[4px] text-start">
          <PermissionRow
            icon={<Mic size={20} aria-hidden="true" />}
            title={t("onboarding.permissions.microphone.name")}
            why={t("onboarding.permissions.microphone.why")}
            status={permissions.microphone}
            next={next === "microphone"}
            action={t(
              permissions.microphoneAsked && isMac
                ? "onboarding.permissions.openSettings"
                : "onboarding.permissions.allow",
            )}
            instruction={micInSettings ? turnOn : undefined}
            onRequest={() => {
              setMicInSettings(!isMac || permissions.microphoneAsked);
              permissions.requestMicrophone();
            }}
          />
          {isMac && (
            <PermissionRow
              icon={<Keyboard size={20} aria-hidden="true" />}
              title={t("onboarding.permissions.accessibility.name")}
              why={t("onboarding.permissions.accessibility.why")}
              status={permissions.accessibility}
              next={next === "accessibility"}
              action={t("onboarding.permissions.allow")}
              instruction={turnOn}
              onRequest={permissions.requestAccessibility}
            />
          )}
        </div>
        <p
          className={`mt-[18px] mb-0 text-small text-ink-3 ${
            isMac && permissions.accessibility === "granted"
              ? "visible"
              : "invisible"
          }`}
        >
          <Trans
            i18nKey="onboarding.permissions.relaunch"
            components={{ relaunch: <TextLink onClick={relaunchHandy} /> }}
          />
        </p>
      </ScreenMain>
    </OnboardingShell>
  );
};
