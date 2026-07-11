import React from "react";
import { ProgressBar, ProgressData } from "../shared";

interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
  percentage: number;
}

interface DownloadStats {
  startTime: number;
  lastUpdate: number;
  totalDownloaded: number;
  speed: number;
}

interface DownloadProgressDisplayProps {
  downloadProgress: Record<string, DownloadProgress>;
  downloadStats: Record<string, DownloadStats>;
  className?: string;
}

const DownloadProgressDisplay: React.FC<DownloadProgressDisplayProps> = ({
  downloadProgress,
  downloadStats,
  className = "",
}) => {
  const progressValues = Object.values(downloadProgress);
  if (progressValues.length === 0) {
    return null;
  }

  const progressData: ProgressData[] = progressValues.map((progress) => {
    const stats = downloadStats[progress.model_id];
    return {
      id: progress.model_id,
      percentage: progress.percentage,
      speed: stats?.speed,
    };
  });

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
