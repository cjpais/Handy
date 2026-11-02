import React from "react";
import { ProgressBar, ProgressData } from "../shared";
import type { DownloadProgress } from "@/stores/modelStore";
import { BYTES_PER_MB } from "@/lib/utils/format";

interface DownloadProgressDisplayProps {
  downloadProgress: Record<string, DownloadProgress>;
  className?: string;
}

const DownloadProgressDisplay: React.FC<DownloadProgressDisplayProps> = ({
  downloadProgress,
  className = "",
}) => {
  const progressValues = Object.values(downloadProgress);
  if (progressValues.length === 0) {
    return null;
  }

  const progressData: ProgressData[] = progressValues.map((progress) => ({
    id: progress.model_id,
    percentage: progress.percentage,
    speed:
      progress.bytes_per_sec != null
        ? progress.bytes_per_sec / BYTES_PER_MB
        : undefined,
  }));

  return (
    <ProgressBar
      progress={progressData}
      className={className}
      showSpeed={progressValues.length === 1}
      size="medium"
    />
  );
};

export default DownloadProgressDisplay;
