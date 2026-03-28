import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { FileAudio, UploadCloud } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/Button";

interface StudioDropzoneProps {
  disabled?: boolean;
  supportedExtensions: string[];
  onFileSelected: (path: string) => void | Promise<void>;
}

export const StudioDropzone: React.FC<StudioDropzoneProps> = ({
  disabled = false,
  supportedExtensions,
  onFileSelected,
}) => {
  const { t } = useTranslation();
  const dragCounter = useRef(0);
  const [dragging, setDragging] = React.useState(false);

  const chooseFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Audio", extensions: supportedExtensions }],
    });

    if (typeof selected === "string") {
      await onFileSelected(selected);
    }
  };

  // Tauri's webview extends the standard File object with a `path` property
  // on drag-and-drop. This is not part of the Web API and may be unavailable
  // on some platforms (e.g. certain Linux/Wayland configs). The fallback toast
  // guides the user to "Choose File" via the native dialog instead.
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
          return;
        }
        toast.error(
          t("studio.dropzone.dropUnavailable", {
            defaultValue:
              "This drop could not be read here. Use Choose File instead.",
          }),
        );
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
      <h2 className="text-lg font-semibold">
        {t("studio.dropzone.title", { defaultValue: "Drop an audio file" })}
      </h2>
      <p className="mt-2 text-sm text-text/60">
        {t("studio.dropzone.description", {
          defaultValue:
            "One file at a time. Studio keeps the flow simple and reliable.",
        })}
      </p>
      <Button
        onClick={chooseFile}
        variant="primary-soft"
        className="mt-5"
        disabled={disabled}
      >
        {t("studio.dropzone.chooseFile", { defaultValue: "Choose File" })}
      </Button>
      <p className="mt-4 text-xs text-text/50">
        {t("studio.dropzone.supported", {
          defaultValue: "Supported: {{extensions}}",
          extensions: supportedExtensions.map((extension) => extension.toUpperCase()).join(", "),
        })}
      </p>
    </div>
  );
};
