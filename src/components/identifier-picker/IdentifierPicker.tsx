/**
 * IdentifierPicker
 *
 * A floating overlay that appears when the backend emits an
 * `identifier-pick-needed` event.  It shows one card per ambiguous token and
 * lets the user select the correct replacement (or dismiss to keep the
 * original).  Once all selections are made, it sends them back to the backend
 * via the `confirm_identifier_pick` command.
 *
 * The backend blocks for up to 10 s waiting for this response, so the picker
 * must appear and respond quickly.
 */

import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, events } from "@/bindings";

interface PickItem {
  token: string;
  candidates: string[];
}

interface PickRequest {
  request_id: string;
  items: PickItem[];
}

/** Per-token selection state: either a chosen candidate or null (keep original). */
type Selections = Record<string, string | null>;

export const IdentifierPicker: React.FC = () => {
  const { t } = useTranslation();
  const [request, setRequest] = useState<PickRequest | null>(null);
  const [selections, setSelections] = useState<Selections>({});
  const [countdown, setCountdown] = useState(10);

  // ---- Listen for picker events from backend ----
  useEffect(() => {
    const unlisten = events.identifierPickNeededEvent.listen((event) => {
      const payload = event.payload as PickRequest;
      setRequest(payload);
      // Default selection: first candidate for each token.
      const defaults: Selections = {};
      for (const item of payload.items) {
        defaults[item.token] = item.candidates[0] ?? null;
      }
      setSelections(defaults);
      setCountdown(10);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // ---- Countdown timer: auto-submit top picks on timeout ----
  useEffect(() => {
    if (!request) return;
    if (countdown <= 0) {
      handleConfirm();
      return;
    }
    const id = window.setTimeout(() => setCountdown((c) => c - 1), 1000);
    return () => window.clearTimeout(id);
  }, [request, countdown]);

  const handleSelect = (token: string, candidate: string) => {
    setSelections((prev) => ({ ...prev, [token]: candidate }));
  };

  const handleKeepOriginal = (token: string) => {
    // Sending the original token as the selection keeps it unchanged.
    setSelections((prev) => ({ ...prev, [token]: token }));
  };

  const handleConfirm = useCallback(async () => {
    if (!request) return;

    // Build the selections map: token → chosen replacement (fall back to token itself).
    const finalSelections: Record<string, string> = {};
    for (const item of request.items) {
      finalSelections[item.token] =
        selections[item.token] ?? item.token;
    }

    try {
      await commands.confirmIdentifierPick(request.request_id, finalSelections);
    } catch (e) {
      // Best-effort; backend will timeout gracefully if this fails.
      console.error("Failed to confirm identifier pick:", e);
    }

    setRequest(null);
    setSelections({});
  }, [request, selections]);

  const handleDismiss = useCallback(async () => {
    if (!request) return;
    // Return each token unchanged.
    const originals: Record<string, string> = {};
    for (const item of request.items) {
      originals[item.token] = item.token;
    }
    try {
      await commands.confirmIdentifierPick(request.request_id, originals);
    } catch (_) {}
    setRequest(null);
    setSelections({});
  }, [request]);

  if (!request) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center pb-6 pointer-events-none"
      aria-label={t("identifierPicker.ariaLabel")}
    >
      <div
        className="pointer-events-auto bg-gray-900/95 backdrop-blur-sm border border-white/10 rounded-2xl shadow-2xl p-4 max-w-lg w-full mx-4 space-y-3 animate-in slide-in-from-bottom-4 duration-200"
        role="dialog"
        aria-modal="true"
      >
        {/* Header */}
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-white/90">
            {t("identifierPicker.title")}
          </h2>
          <span className="text-xs text-white/40 tabular-nums">
            {t("identifierPicker.countdown", { seconds: countdown })}
          </span>
        </div>

        {/* One card per ambiguous token */}
        {request.items.map((item) => (
          <div
            key={item.token}
            className="bg-white/5 rounded-lg p-3 space-y-2"
          >
            <p className="text-xs text-white/50">
              {t("identifierPicker.prompt", { token: item.token })}
            </p>
            <div className="flex flex-wrap gap-1.5">
              {item.candidates.map((candidate) => (
                <button
                  key={candidate}
                  onClick={() => handleSelect(item.token, candidate)}
                  className={`px-2.5 py-1 rounded-md text-xs font-mono transition-colors ${
                    selections[item.token] === candidate
                      ? "bg-logo-primary text-white"
                      : "bg-white/10 text-white/80 hover:bg-white/20"
                  }`}
                >
                  {candidate}
                </button>
              ))}
              {/* Option to keep the original spoken word unchanged */}
              <button
                onClick={() => handleKeepOriginal(item.token)}
                className={`px-2.5 py-1 rounded-md text-xs transition-colors ${
                  selections[item.token] === item.token
                    ? "bg-white/20 text-white"
                    : "bg-transparent text-white/40 hover:text-white/60 hover:bg-white/5"
                }`}
              >
                {t("identifierPicker.keepOriginal")}
              </button>
            </div>
          </div>
        ))}

        {/* Action buttons */}
        <div className="flex justify-end gap-2 pt-1">
          <button
            onClick={handleDismiss}
            className="px-3 py-1.5 rounded-lg text-xs text-white/50 hover:text-white/80 hover:bg-white/5 transition-colors"
          >
            {t("identifierPicker.dismiss")}
          </button>
          <button
            onClick={handleConfirm}
            className="px-3 py-1.5 rounded-lg text-xs font-medium bg-logo-primary text-white hover:opacity-90 transition-opacity"
          >
            {t("identifierPicker.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
};
