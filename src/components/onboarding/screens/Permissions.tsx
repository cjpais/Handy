import React from "react";
import { Trans, useTranslation } from "react-i18next";
import { relaunch } from "@tauri-apps/plugin-process";
import { Button } from "../../ui/Button";
import { Spinner } from "../../ui/Spinner";
import { TextLink } from "../../ui/TextLink";
import { DoneMark } from "../../shared/DoneMark";
import {
  OnboardingShell,
  ScreenMain,
  ScreenSub,
  ScreenTitle,
} from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import type { Permissions as PermissionsState } from "../usePermissions";

const MicIcon = () => (
  <svg
    width="20"
    height="20"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="9" y="2" width="6" height="12" rx="3" />
    <path d="M5 10a7 7 0 0 0 14 0M12 17v4" />
  </svg>
);

const KeyboardIcon = () => (
  <svg
    width="20"
    height="20"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="2" y="6" width="20" height="13" rx="2" />
    <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 15h10" />
  </svg>
);

interface PermissionRowProps {
  icon: React.ReactNode;
  title: string;
  why: string;
  status: PermissionsState["microphone"];
  /** The next one to allow gets the filled button. */
  next: boolean;
  action: string;
  waiting: string;
  onRequest: () => void;
}

const PermissionRow: React.FC<PermissionRowProps> = ({
  icon,
  title,
  why,
  status,
  next,
  action,
  waiting,
  onRequest,
}) => {
  const { t } = useTranslation();
  const granted = status === "granted";
  return (
    <div className="grid grid-cols-[46px_1fr_auto] items-center gap-[18px] py-[18px] [&+&]:border-t [&+&]:border-line">
      <span
        className={`grid size-[46px] place-items-center rounded-[11px] border-[1.5px] transition-[border-color,color] duration-[250ms] ${
          granted
            ? "border-moss bg-background text-moss"
            : "border-plum-edge bg-key-face text-key-ink"
        }`}
      >
        {icon}
      </span>
      <div>
        <b className="block font-display text-row font-[650] leading-[1.3]">
          {title}
        </b>
        <p className="mt-[2px] mb-0 text-small leading-[1.45] text-ink-2 text-pretty">
          {why}
        </p>
      </div>
      {granted ? (
        <span className="inline-flex items-center gap-[6px] text-small font-semibold text-moss">
          <DoneMark />
          {t("onboarding.permissions.allowed")}
        </span>
      ) : status === "waiting" ? (
        <span className="inline-flex max-w-[130px] items-center gap-[6px] text-label leading-[1.35] text-ink-3">
          <Spinner className="shrink-0" />
          <span>{waiting}</span>
        </span>
      ) : (
        <Button
          variant={next ? "primary" : "outline"}
          size="md"
          onClick={onRequest}
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

/**
 * Screen 2, where the OS asks: the microphone, and Accessibility on macOS.
 * Each row asks for its permission; the next one to allow is filled.
 */
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
      <ScreenMain task>
        {welcomeBack && (
          <div className="mb-[18px] flex animate-rise items-center gap-[10px] text-small font-semibold text-moss">
            <DoneMark />
            {t("onboarding.permissions.welcomeBack")}
          </div>
        )}
        <ScreenTitle task={!welcomeBack}>
          {t(
            isMac
              ? "onboarding.permissions.titleMac"
              : "onboarding.permissions.titleWindows",
          )}
        </ScreenTitle>
        <ScreenSub>
          {t(
            isMac
              ? "onboarding.permissions.subtitleMac"
              : "onboarding.permissions.subtitleWindows",
          )}
        </ScreenSub>
        <div className="mt-[28px] flex w-full flex-col rounded-panel border-[1.5px] border-line-strong px-[22px] py-[4px] text-start">
          <PermissionRow
            icon={<MicIcon />}
            title={t("onboarding.permissions.microphone.name")}
            why={t("onboarding.permissions.microphone.why")}
            status={permissions.microphone}
            next={next === "microphone"}
            action={t(
              permissions.microphoneAsked && isMac
                ? "onboarding.permissions.openSettings"
                : "onboarding.permissions.allow",
            )}
            waiting={t(
              isMac
                ? "onboarding.permissions.microphone.waitingMac"
                : "onboarding.permissions.microphone.waitingWindows",
            )}
            onRequest={permissions.requestMicrophone}
          />
          {isMac && (
            <PermissionRow
              icon={<KeyboardIcon />}
              title={t("onboarding.permissions.accessibility.name")}
              why={t("onboarding.permissions.accessibility.why")}
              status={permissions.accessibility}
              next={next === "accessibility"}
              action={t("onboarding.permissions.openSettings")}
              waiting={t("onboarding.permissions.accessibility.waiting")}
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
            components={{ link: <TextLink onClick={relaunchHandy} /> }}
          />
        </p>
      </ScreenMain>
    </OnboardingShell>
  );
};
