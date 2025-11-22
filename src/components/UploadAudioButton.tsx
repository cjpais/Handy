import React, { useState } from "react";
// Tauri v2: dialog + core APIs moved to plugins/core packages
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";

// Simple front-end only implementation for selecting an audio file
// and invoking the backend transcription command. Backend command
// `transcribe_file` will be implemented later.
export const UploadAudioButton: React.FC = () => {
  const [status, setStatus] = useState<string>("");
  const [lastFile, setLastFile] = useState<string | null>(null);

  const handleSelect = async () => {
    setStatus("");
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Audio",
            extensions: ["mp3", "wav", "m4a", "flac", "ogg", "aac"],
          },
        ],
      });
      if (!selected || Array.isArray(selected)) return;
      setLastFile(selected);
      setStatus("Sending to transcription…");
      try {
        await invoke("transcribe_file", { filePath: selected });
        setStatus("Queued for transcription");
      } catch (err: any) {
        setStatus(`Failed to invoke: ${err?.message || String(err)}`);
      }
    } catch (e: any) {
      setStatus(`Dialog error: ${e?.message || String(e)}`);
    }
  };

  return (
    <div className="flex flex-col w-full items-center gap-1 pt-3 mt-3 border-t border-mid-gray/20">
      <button
        onClick={handleSelect}
        className="w-full text-sm font-medium p-2 rounded-lg bg-logo-primary/80 hover:bg-logo-primary transition-colors"
      >
        Upload MP3 / Audio
      </button>
      {status && (
        <p className="text-xs text-center px-2 opacity-80">
          {status}
          {lastFile && status !== "Sending to transcription…" && (
            <span className="block truncate max-w-full" title={lastFile}>
              {lastFile}
            </span>
          )}
        </p>
      )}
    </div>
  );
};

export default UploadAudioButton;