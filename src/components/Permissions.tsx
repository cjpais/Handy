import React from "react";
import { useTranslation } from "react-i18next";
import { Keyboard, Mic } from "lucide-react";
import { Button } from "./ui/Button";
import { Spinner } from "./ui/Spinner";
import type { PermissionStatus, Permissions } from "../hooks/usePermissions";

interface PermissionRowProps {
  icon: React.ReactNode;
  title: string;
  why: string;
  status: PermissionStatus;
  /** Its turn: the rows are allowed in order, so only this one is live. */
  next: boolean;
  action: string;
  onRequest: () => void;
}

const PermissionRow: React.FC<PermissionRowProps> = ({
  icon,
  title,
  why,
  status,
  next,
  action,
  onRequest,
}) => {
  const { t } = useTranslation();
  return (
    <div className="grid grid-cols-[46px_1fr_minmax(112px,auto)] items-center gap-4 py-4 [&+&]:border-t [&+&]:border-line">
      <span className="grid size-[46px] place-items-center rounded-lg bg-wash text-ink-2">
        {icon}
      </span>
      <div>
        <b className="block font-semibold">{title}</b>
        <p className="mt-0.5 mb-0 text-sm text-ink-2">{why}</p>
      </div>
      {status === "granted" ? (
        <span className="justify-self-end text-sm font-semibold text-moss">
          {t("permissions.allowed")}
        </span>
      ) : status === "waiting" ? (
        <span className="inline-flex items-center justify-self-end gap-1.5 text-sm text-ink-3">
          <Spinner className="shrink-0" />
          {t("permissions.waiting")}
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

/** The one screen before the settings window: allow the microphone, then typing. */
export const PermissionsScreen: React.FC<{ permissions: Permissions }> = ({
  permissions,
}) => {
  const { t } = useTranslation();
  const isMac = permissions.platform === "macos";
  return (
    <div className="h-screen flex items-center justify-center p-6 select-none cursor-default">
      <div className="w-full max-w-lg">
        <h1 className="text-xl font-semibold">
          {isMac ? t("permissions.titleMac") : t("permissions.titleWindows")}
        </h1>
        <p className="mt-1 mb-4 text-sm text-ink-2">
          {isMac
            ? t("permissions.subtitleMac")
            : t("permissions.subtitleWindows")}
        </p>
        <div className="rounded-lg border border-line px-4">
          <PermissionRow
            icon={<Mic size={22} />}
            title={t("permissions.microphone.name")}
            why={t("permissions.microphone.why")}
            status={permissions.microphone}
            next
            action={
              permissions.microphoneAsked
                ? t("permissions.openSettings")
                : t("permissions.allow")
            }
            onRequest={permissions.requestMicrophone}
          />
          {isMac && (
            <PermissionRow
              icon={<Keyboard size={22} />}
              title={t("permissions.accessibility.name")}
              why={t("permissions.accessibility.why")}
              status={permissions.accessibility}
              next={permissions.microphone === "granted"}
              action={t("permissions.allow")}
              onRequest={permissions.requestAccessibility}
            />
          )}
        </div>
      </div>
    </div>
  );
};
