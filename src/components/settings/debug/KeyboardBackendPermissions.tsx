import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy } from "lucide-react";
import { toast } from "sonner";
import { commands, events, type KeyboardBackendStatus } from "@/bindings";
import { Alert } from "../../ui/Alert";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { useOsType } from "../../../hooks/useOsType";
import { useSettings } from "../../../hooks/useSettings";

// Copy-paste fixes for the two permission tiers of the Linux evdev backend.
// Keep in sync with contrib/udev/70-handy-keys.rules and the handy-keys docs:
// reading hotkeys needs /dev/input access ('input' group), blocking them
// additionally needs /dev/uinput write access (udev uaccess rule).
const READ_ACCESS_COMMAND = "sudo usermod -aG input $USER";
const BLOCKING_ACCESS_COMMAND =
  'echo \'KERNEL=="uinput", SUBSYSTEM=="misc", TAG+="uaccess", OPTIONS+="static_node=uinput"\' | sudo tee /etc/udev/rules.d/70-handy-keys.rules && sudo udevadm control --reload && sudo udevadm trigger';

const CommandRow: React.FC<{ label: string; command: string }> = ({
  label,
  command,
}) => {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Failed to copy to clipboard:", err);
    }
  };

  return (
    <div className="space-y-1">
      <p className="text-xs text-mid-gray">{label}</p>
      <div className="flex items-start gap-2">
        <code className="flex-1 text-xs font-mono bg-mid-gray/10 border border-mid-gray/20 rounded px-2 py-1.5 break-all select-text">
          {command}
        </code>
        <Button
          variant="secondary"
          size="sm"
          onClick={handleCopy}
          title={
            copied
              ? t("settings.debug.keyboardImplementation.permissions.copied")
              : t("settings.debug.keyboardImplementation.permissions.copy")
          }
        >
          {copied ? (
            <Check className="w-3.5 h-3.5" />
          ) : (
            <Copy className="w-3.5 h-3.5" />
          )}
        </Button>
      </div>
    </div>
  );
};

interface PermissionsFixProps {
  /** "error": handy-keys could not start at all; "degraded": detect-only */
  mode: "error" | "degraded";
  /** Raw backend error naming the exact failure, shown for transparency */
  detail?: string | null;
}

const PermissionsFixContent: React.FC<PermissionsFixProps> = ({
  mode,
  detail,
}) => {
  const { t } = useTranslation();
  const keys = "settings.debug.keyboardImplementation.permissions";

  return (
    <div className="space-y-3">
      <Alert variant={mode === "error" ? "error" : "warning"}>
        {mode === "error"
          ? t(`${keys}.initErrorIntro`)
          : t(`${keys}.degradedIntro`)}
      </Alert>
      {mode === "error" ? (
        <>
          <CommandRow
            label={t(`${keys}.readCommandLabel`)}
            command={READ_ACCESS_COMMAND}
          />
          <CommandRow
            label={t(`${keys}.blockingCommandLabel`)}
            command={BLOCKING_ACCESS_COMMAND}
          />
        </>
      ) : (
        <CommandRow
          label={t(`${keys}.blockingCommandLabelDegraded`)}
          command={BLOCKING_ACCESS_COMMAND}
        />
      )}
      <p className="text-xs text-mid-gray">{t(`${keys}.packagedNote`)}</p>
      {detail && (
        <p className="text-xs text-mid-gray font-mono break-all">
          {t(`${keys}.errorDetails`)}: {detail}
        </p>
      )}
    </div>
  );
};

/** Retry the handy-keys backend and toast the outcome. */
const useRetryBackend = (
  onSuccess?: (status: KeyboardBackendStatus) => void,
) => {
  const { t } = useTranslation();
  const { refreshSettings } = useSettings();
  const [retrying, setRetrying] = useState(false);
  const keys = "settings.debug.keyboardImplementation.permissions";

  const retry = async () => {
    setRetrying(true);
    try {
      const result = await commands.retryHandyKeysBackend();
      if (result.status === "error") {
        toast.error(String(result.error));
      } else {
        if (result.data.handy_keys?.blocking) {
          toast.success(t(`${keys}.retrySuccess`));
        } else {
          toast.warning(t(`${keys}.retryStillDegraded`));
        }
        onSuccess?.(result.data);
      }
      // The retry may have switched keyboard_implementation back to handy_keys
      await refreshSettings();
    } catch (error) {
      console.error("Failed to retry handy-keys backend:", error);
      toast.error(String(error));
    } finally {
      setRetrying(false);
    }
  };

  return { retry, retrying };
};

interface KeyboardPermissionsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The init error returned when switching to handy-keys failed */
  error: string | null;
}

/**
 * Setup dialog shown when switching to the Handy Keys backend fails on Linux
 * because input devices are not accessible. Renders the exact fix commands
 * with copy buttons and a retry that works without restarting the app.
 */
export const KeyboardPermissionsDialog: React.FC<
  KeyboardPermissionsDialogProps
> = ({ open, onOpenChange, error }) => {
  const { t } = useTranslation();
  const keys = "settings.debug.keyboardImplementation.permissions";
  const { retry, retrying } = useRetryBackend((status) => {
    // Only dismiss once handy-keys is actually driving shortcuts again
    if (status.active_implementation === "handy_keys") {
      onOpenChange(false);
    }
  });

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={t(`${keys}.dialogTitle`)}
      closeLabel={t(`${keys}.close`)}
      footer={
        <div className="flex justify-end gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t(`${keys}.close`)}
          </Button>
          <Button size="sm" onClick={retry} disabled={retrying}>
            {retrying ? t(`${keys}.retrying`) : t(`${keys}.retry`)}
          </Button>
        </div>
      }
    >
      <PermissionsFixContent mode="error" detail={error} />
    </Dialog>
  );
};

/**
 * Inline banner for the settings page (Linux only). Surfaces the two
 * missing-permission states of the evdev backend instead of letting hotkeys
 * fail silently: init failed entirely (with the full fix), or running
 * detect-only because /dev/uinput is not writable (hotkeys are seen but not
 * swallowed).
 */
export const KeyboardBackendPermissions: React.FC = () => {
  const { t } = useTranslation();
  const osType = useOsType();
  const keys = "settings.debug.keyboardImplementation.permissions";
  const [status, setStatus] = useState<KeyboardBackendStatus | null>(null);
  const { retry, retrying } = useRetryBackend();

  const refresh = useCallback(async () => {
    try {
      setStatus(await commands.getKeyboardBackendStatus());
    } catch (error) {
      console.error("Failed to fetch keyboard backend status:", error);
    }
  }, []);

  useEffect(() => {
    if (osType !== "linux") return;
    refresh();
    const unlisten = events.keyboardBackendStatus.listen((event) => {
      setStatus(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [osType, refresh]);

  if (osType !== "linux" || !status) return null;

  const failed = status.init_error != null;
  const degraded =
    !failed &&
    status.active_implementation === "handy_keys" &&
    status.handy_keys != null &&
    !status.handy_keys.blocking;

  if (!failed && !degraded) return null;

  return (
    <div className="p-4 space-y-3">
      <PermissionsFixContent
        mode={failed ? "error" : "degraded"}
        detail={failed ? status.init_error : status.handy_keys?.blocking_error}
      />
      <Button size="sm" onClick={retry} disabled={retrying}>
        {retrying ? t(`${keys}.retrying`) : t(`${keys}.retry`)}
      </Button>
    </div>
  );
};
