import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { X } from "lucide-react";
import { commands, type SecureInputStatus } from "@/bindings";
import { Button } from "./ui/Button";

// Detailed remediation steps live in the docs rather than in the banner
export const SECURE_INPUT_HELP_URL =
  "https://handy.computer/docs/troubleshooting#shortcuts-stopped-working-on-macos-secure-input";

/**
 * Compact warning banner shown while macOS Secure Input is stuck on.
 *
 * Secure Input (password fields, Terminal's "Secure Keyboard Entry", a stuck
 * loginwindow) blocks key events from reaching Handy's keyboard listener, so
 * keyed shortcuts silently stop firing (issue #1578). The backend monitor
 * emits `secure-input-changed` on state transitions; `sustained` filters out
 * the normal momentary activation from focusing a password field.
 */
const SecureInputWarning: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<SecureInputStatus | null>(null);
  const [dismissed, setDismissed] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await commands.getSecureInputStatus());
    } catch (e) {
      console.warn("Failed to fetch secure input status:", e);
    }
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen<SecureInputStatus>(
      "secure-input-changed",
      (event) => setStatus(event.payload),
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh]);

  // Only warn when the user is actually impacted: a binding is degraded
  // (side-specific matching widened) or dead (e.g. fn+key), or they ran into
  // the blocked shortcut recorder. When the fallback covers everything
  // transparently — and nothing else surfaced — stay silent; the backend
  // still logs.
  const impacted =
    status !== null &&
    ((status.sustained &&
      (status.degraded_bindings.length > 0 ||
        status.uncovered_bindings.length > 0)) ||
      status.recorder_blocked);

  // A dismissal lasts for the current episode only: once the condition
  // clears, the next occurrence warns again. The tray badge is the
  // persistent indicator and is not dismissible.
  useEffect(() => {
    if (!impacted) {
      setDismissed(false);
    }
  }, [impacted]);

  if (!impacted || dismissed) {
    return null;
  }

  const bindingNames = (ids: string[]) =>
    ids
      .map((id) => t(`settings.general.shortcut.bindings.${id}.name`, id))
      .join(", ");

  const description =
    status.culprit_name !== null
      ? t("secureInput.descriptionWithCulprit", {
          name: status.culprit_name,
          pid: status.culprit_pid,
        })
      : t("secureInput.descriptionNoCulprit");

  const statusLines = [
    status.uncovered_bindings.length > 0
      ? t("secureInput.uncovered", {
          bindings: bindingNames(status.uncovered_bindings),
        })
      : null,
    status.degraded_bindings.length > 0
      ? t("secureInput.degraded", {
          bindings: bindingNames(status.degraded_bindings),
        })
      : null,
    status.recorder_blocked ? t("secureInput.recorderDisabled") : null,
  ].filter((line): line is string => line !== null);

  return (
    <div className="p-4 w-full rounded-lg border border-warning/70 bg-warning/10 space-y-2">
      <div className="flex items-start justify-between gap-2">
        <p className="text-sm font-semibold">{t("secureInput.title")}</p>
        <button
          onClick={() => setDismissed(true)}
          aria-label={t("secureInput.dismiss")}
          className="shrink-0 rounded p-0.5 text-mid-gray hover:text-text cursor-pointer"
        >
          <X className="w-4 h-4" />
        </button>
      </div>
      <p className="text-sm">
        {description} {statusLines.join(" ")}
      </p>
      <div>
        <Button
          variant="warning"
          size="sm"
          onClick={() => openUrl(SECURE_INPUT_HELP_URL)}
        >
          {t("secureInput.learnMore")}
        </Button>
      </div>
    </div>
  );
};

export default SecureInputWarning;
