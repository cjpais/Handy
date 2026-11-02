import { open } from "@tauri-apps/plugin-dialog";
import { notify } from "@/stores/noticeStore";
import i18n from "@/i18n";
import { commands } from "@/bindings";

// What the file picker offers. Dropped files of any type are still tried.
// No ogg/oga/opus/webm: they're mostly Opus (voice notes), which can't be
// decoded yet, though Vorbis .ogg works if dropped.
const MEDIA_EXTENSIONS = [
  "mp3",
  "m4a",
  "aac",
  "wav",
  "flac",
  "aif",
  "aiff",
  "mp4",
  "m4v",
  "mov",
];

const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

// Adds each file to History and queues it to be transcribed. Resolves to
// how many were added.
export const transcribeFiles = async (paths: string[]): Promise<number> => {
  let added = 0;
  for (const path of paths) {
    // Its audio is read in full before it appears in History.
    const result = await commands.transcribeFile(path);
    if (result.status === "ok") {
      added += 1;
    } else {
      console.error(`Failed to transcribe ${path}:`, result.error);
      // Keyed by file, so each bad file keeps its own row.
      notify({
        title: i18n.t("settings.history.fileUnsupported", {
          name: fileName(path),
        }),
        key: `file:${path}`,
      });
    }
  }
  return added;
};

// Asks for files to transcribe. Resolves to false if nothing was picked.
export const pickAndTranscribeFiles = async (): Promise<boolean> => {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [
      {
        name: i18n.t("settings.history.mediaFiles"),
        extensions: MEDIA_EXTENSIONS,
      },
    ],
  });
  if (!selected) return false;
  await transcribeFiles(Array.isArray(selected) ? selected : [selected]);
  return true;
};
