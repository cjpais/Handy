import React, { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import { Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
} from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { LocalFileTranscriber } from "@/components/LocalFileTranscriber";
import { MeetingEntryComponent } from "@/components/settings/meetings/MeetingsSettings";

// ---------------------------------------------------------------------------
// Date-grouping helpers
// ---------------------------------------------------------------------------

function toMidnightMs(timestampSeconds: number): number {
  const d = new Date(timestampSeconds * 1000);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

function formatGroupLabel(timestampSeconds: number, locale: string): string {
  const entryMidnight = toMidnightMs(timestampSeconds);
  const now = new Date();
  const todayMidnight = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  ).getTime();

  if (entryMidnight === todayMidnight) return "Today";
  if (entryMidnight === todayMidnight - 86_400_000) return "Yesterday";

  const d = new Date(timestampSeconds * 1000);
  return d
    .toLocaleDateString(locale, {
      weekday: "short",
      month: "short",
      day: "numeric",
    })
    .toUpperCase();
}

interface DateGroup {
  label: string;
  entries: HistoryEntry[];
}

function groupByDate(entries: HistoryEntry[], locale: string): DateGroup[] {
  const groups: DateGroup[] = [];
  const seen = new Map<number, DateGroup>();

  for (const entry of entries) {
    const midnight = toMidnightMs(entry.timestamp);
    if (!seen.has(midnight)) {
      const group: DateGroup = {
        label: formatGroupLabel(entry.timestamp, locale),
        entries: [],
      };
      seen.set(midnight, group);
      groups.push(group);
    }
    seen.get(midnight)!.entries.push(entry);
  }

  return groups;
}

// ---------------------------------------------------------------------------
// MeetingsView
// ---------------------------------------------------------------------------

const MEETING_PROMPTS = [
  "default_meeting_summary",
  "default_meeting_notes_with_actions",
] as const;

function isMeetingEntry(entry: HistoryEntry): boolean {
  return MEETING_PROMPTS.includes(
    entry.post_process_prompt as (typeof MEETING_PROMPTS)[number],
  );
}

export const MeetingsView: React.FC = () => {
  const { t, i18n } = useTranslation();
  const osType = useOsType();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [transcriberFiles, setTranscriberFiles] = useState<string[]>([]);
  const [googleStatus, setGoogleStatus] = useState<{
    gmail_tasks_connected: boolean;
  } | null>(null);

  // -------------------------------------------------------------------------
  // Data loading
  // -------------------------------------------------------------------------

  const loadMeetings = useCallback(async () => {
    setLoading(true);
    try {
      const [status, historyResult] = await Promise.allSettled([
        commands.getGoogleIntegrationStatus(),
        commands.getHistoryEntries(null, 100),
      ]);

      if (status.status === "fulfilled") {
        setGoogleStatus(status.value);
      }

      if (
        historyResult.status === "fulfilled" &&
        historyResult.value.status === "ok"
      ) {
        setEntries(historyResult.value.data.entries.filter(isMeetingEntry));
      }
    } catch (error) {
      console.error("Failed to load meetings:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadMeetings();
  }, [loadMeetings]);

  // Live updates from the transcription pipeline
  useEffect(() => {
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;

      if (payload.action === "added") {
        if (isMeetingEntry(payload.entry)) {
          setEntries((prev) => [payload.entry, ...prev]);
        }
      } else if (payload.action === "updated") {
        if (isMeetingEntry(payload.entry)) {
          setEntries((prev) =>
            prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
          );
        }
      } else if (payload.action === "deleted") {
        setEntries((prev) => prev.filter((e) => e.id !== payload.id));
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // -------------------------------------------------------------------------
  // Actions
  // -------------------------------------------------------------------------

  const getAudioUrl = useCallback(
    async (fileName: string): Promise<string | null> => {
      try {
        const result = await commands.getAudioFilePath(fileName);
        if (result.status !== "ok") return null;

        if (osType === "linux") {
          const fileData = await readFile(result.data);
          const blob = new Blob([fileData], { type: "audio/wav" });
          return URL.createObjectURL(blob);
        }

        return convertFileSrc(result.data, "asset");
      } catch {
        return null;
      }
    },
    [osType],
  );

  const deleteMeeting = useCallback(
    async (id: number) => {
      setEntries((prev) => prev.filter((e) => e.id !== id));
      try {
        const result = await commands.deleteHistoryEntry(id);
        if (result.status !== "ok") void loadMeetings();
      } catch {
        void loadMeetings();
      }
    },
    [loadMeetings],
  );

  const handleUploadClick = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          { name: "Audio", extensions: ["wav", "mp3", "m4a", "flac", "ogg"] },
        ],
      });
      if (selected) {
        setTranscriberFiles(Array.isArray(selected) ? selected : [selected]);
      }
    } catch (error) {
      console.error("Failed to open file dialog:", error);
    }
  };

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------

  const groups = groupByDate(entries, i18n.language);
  const isGoogleConnected = !!googleStatus?.gmail_tasks_connected;

  return (
    <div className="w-full">
      {/* Top bar: section label + upload button */}
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-bark-grey">
          {t("settings.meetings.title")}
        </h2>
        <button
          type="button"
          onClick={handleUploadClick}
          className="flex items-center gap-1.5 text-xs font-medium text-forest-green hover:text-deep-forest-green transition-colors bg-forest-green/10 hover:bg-forest-green/20 px-3 py-1.5 rounded-lg"
        >
          <Upload className="w-3.5 h-3.5" />
          {t("settings.meetings.uploadAudio")}
        </button>
      </div>

      {/* Content */}
      {loading ? (
        <div className="py-16 text-center text-bark-grey text-sm">
          {t("settings.meetings.loading")}
        </div>
      ) : entries.length === 0 ? (
        <div className="py-16 text-center text-bark-grey text-sm">
          {t("settings.meetings.empty")}
        </div>
      ) : (
        <div className="space-y-6">
          {groups.map((group) => (
            <section key={group.label}>
              {/* Date group header */}
              <p className="text-[10px] font-mono font-semibold uppercase tracking-[0.14em] text-bark-grey mb-2 px-1">
                {group.label}
              </p>

              {/* Meeting cards for this date */}
              <div className="bg-orange-off-white border border-stone-mist rounded-xl overflow-hidden">
                <div className="divide-y divide-stone-mist">
                  {group.entries.map((entry) => (
                    <MeetingEntryComponent
                      key={entry.id}
                      entry={entry}
                      getAudioUrl={getAudioUrl}
                      deleteMeeting={deleteMeeting}
                      isGoogleConnected={isGoogleConnected}
                    />
                  ))}
                </div>
              </div>
            </section>
          ))}
        </div>
      )}

      {/* Local file transcription modal */}
      {transcriberFiles.length > 0 && (
        <LocalFileTranscriber
          initialFiles={transcriberFiles}
          onClose={() => setTranscriberFiles([])}
          onSuccess={() => {
            void loadMeetings();
          }}
        />
      )}
    </div>
  );
};
