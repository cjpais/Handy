import React from "react";
import { ProgressBar, ProgressData } from "../shared";
import type { DownloadProgress } from "@/stores/modelStore";

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
        ? progress.bytes_per_sec / (1024 * 1024)
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
