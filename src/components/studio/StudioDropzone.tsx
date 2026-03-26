import React, { useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FileAudio, UploadCloud } from "lucide-react";
import { Button } from "@/components/ui/Button";

const MEDIA_EXTENSIONS = [
  "mp3",
  "mp4",
  "wav",
  "m4a",
  "mov",
  "mkv",
  "flac",
  "ogg",
  "webm",
  "avi",
];

interface StudioDropzoneProps {
  disabled?: boolean;
  onFileSelected: (path: string) => void | Promise<void>;
}

export const StudioDropzone: React.FC<StudioDropzoneProps> = ({
  disabled = false,
  onFileSelected,
}) => {
  const dragCounter = useRef(0);
  const [dragging, setDragging] = React.useState(false);

  const chooseFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Media", extensions: MEDIA_EXTENSIONS }],
    });

    if (typeof selected === "string") {
      await onFileSelected(selected);
    }
  };

  const readDroppedPath = (event: React.DragEvent<HTMLDivElement>) => {
    const file = event.dataTransfer.files?.[0] as File & { path?: string };
    return file?.path || null;
  };

  return (
    <div
      onDragEnter={(event) => {
        event.preventDefault();
        dragCounter.current += 1;
        setDragging(true);
      }}
      onDragOver={(event) => {
        event.preventDefault();
      }}
      onDragLeave={(event) => {
        event.preventDefault();
        dragCounter.current -= 1;
        if (dragCounter.current <= 0) {
          setDragging(false);
          dragCounter.current = 0;
        }
      }}
      onDrop={async (event) => {
        event.preventDefault();
        dragCounter.current = 0;
        setDragging(false);
        const path = readDroppedPath(event);
        if (path) {
          await onFileSelected(path);
        }
      }}
      className={`rounded-2xl border-2 border-dashed p-8 text-center transition-colors ${
        dragging
          ? "border-logo-primary bg-logo-primary/10"
          : "border-mid-gray/30 bg-mid-gray/5"
      } ${disabled ? "opacity-60" : ""}`}
    >
      <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-logo-primary/15 text-logo-stroke">
        {dragging ? (
          <UploadCloud className="h-6 w-6" />
        ) : (
          <FileAudio className="h-6 w-6" />
        )}
      </div>
      <h2 className="text-lg font-semibold">Drop an audio or video file</h2>
      <p className="mt-2 text-sm text-text/60">
        One file at a time. Studio keeps the flow simple and reliable.
      </p>
      <Button
        onClick={chooseFile}
        variant="primary-soft"
        className="mt-5"
        disabled={disabled}
      >
        Choose File
      </Button>
      <p className="mt-4 text-xs text-text/50">
        Supported: MP3, MP4, WAV, M4A, MOV, MKV, FLAC, OGG, WEBM, AVI
      </p>
    </div>
  );
};
