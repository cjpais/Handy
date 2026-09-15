import React, { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useSettings } from "../../hooks/useSettings";
import { findReleaseNoteToShow } from "./releaseNotes";
import type { ReleaseNote } from "./releaseNotes";
import { WhatsNewModal } from "./WhatsNewModal";

export const WhatsNewGate: React.FC = () => {
  const { settings, isLoading, updateSetting } = useSettings();
  const [note, setNote] = useState<ReleaseNote | null>(null);
  const [isOpen, setIsOpen] = useState(false);
  const dismissedVersionRef = useRef<string | null>(null);

  useEffect(() => {
    if (isLoading || !settings || !settings.show_whats_new_on_update) {
      setIsOpen(false);
      setNote(null);
      return;
    }

    let cancelled = false;

    const loadReleaseNote = async () => {
      try {
        const currentVersion = await getVersion();
        if (cancelled) return;

        const releaseNote = findReleaseNoteToShow({
          currentVersion,
          lastSeenVersion: settings.whats_new_last_seen_version ?? "",
        });

        if (
          !releaseNote ||
          dismissedVersionRef.current === releaseNote.version
        ) {
          setIsOpen(false);
          setNote(null);
          return;
        }

        setNote(releaseNote);
        setIsOpen(true);
      } catch (error) {
        console.error("Failed to load release notes:", error);
      }
    };

    void loadReleaseNote();

    return () => {
      cancelled = true;
    };
  }, [
    isLoading,
    settings,
    settings?.show_whats_new_on_update,
    settings?.whats_new_last_seen_version,
  ]);

  const dismiss = () => {
    if (!note) return;

    dismissedVersionRef.current = note.version;
    setIsOpen(false);
    void updateSetting("whats_new_last_seen_version", note.version);
  };

  if (!note) return null;

  return <WhatsNewModal note={note} open={isOpen} onDismiss={dismiss} />;
};
