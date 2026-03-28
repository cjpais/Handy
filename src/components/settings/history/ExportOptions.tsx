import React, { useState } from "react";
import { FileText, FileCode } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { HistoryEntry } from "../../../bindings";

interface ExportOptionsProps {
  entry: HistoryEntry;
}

export const ExportOptions: React.FC<ExportOptionsProps> = ({ entry }) => {
  const { t } = useTranslation();
  const [isExporting, setIsExporting] = useState(false);

  const generateFilename = (extension: string) => {
    const sanitizedTitle = entry.title
      .replace(/[^a-zA-Z0-9\s-]/g, "")
      .replace(/\s+/g, "_")
      .substring(0, 50);
    const date = new Date(entry.timestamp * 1000).toISOString().split("T")[0];
    return `${sanitizedTitle}_${date}.${extension}`;
  };

  const downloadBlob = (blob: Blob, filename: string) => {
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  };

  const exportToTxt = () => {
    setIsExporting(true);
    try {
      const content = entry.post_processed_text || entry.transcription_text;
      const blob = new Blob([content], { type: "text/plain" });
      downloadBlob(blob, generateFilename("txt"));
      toast.success(t("settings.history.exportTxt"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setIsExporting(false);
    }
  };

  const exportToMarkdown = () => {
    setIsExporting(true);
    try {
      const date = new Date(entry.timestamp * 1000).toLocaleDateString(
        "en-US",
        {
          weekday: "long",
          year: "numeric",
          month: "long",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        },
      );

      const duration = entry.duration
        ? `${Math.floor(entry.duration / 60)}m ${Math.round(entry.duration % 60)}s`
        : "Unknown duration";

      const content = `# ${entry.title}

**Date:** ${date}  
**Duration:** ${duration}  
**Source:** ${entry.source || "Recording"}

---

## Transcription

${entry.post_processed_text || entry.transcription_text}

---

*Exported from Handy*
`;

      const blob = new Blob([content], { type: "text/markdown" });
      downloadBlob(blob, generateFilename("md"));
      toast.success(t("settings.history.exportMd"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <div className="flex items-center gap-1">
      <button
        onClick={exportToTxt}
        disabled={isExporting}
        className="text-text/50 hover:text-logo-primary transition-colors cursor-pointer p-1"
        title={t("settings.history.exportTxt")}
      >
        <FileText size={14} />
      </button>
      <button
        onClick={exportToMarkdown}
        disabled={isExporting}
        className="text-text/50 hover:text-logo-primary transition-colors cursor-pointer p-1"
        title={t("settings.history.exportMd")}
      >
        <FileCode size={14} />
      </button>
    </div>
  );
};

export default ExportOptions;
