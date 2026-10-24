import React from "react";

export interface ProgressData {
  id: string;
  percentage: number;
  speed?: number;
  label?: string;
}

interface ProgressBarProps {
  progress: ProgressData[];
  className?: string;
  size?: "medium" | "large";
  showSpeed?: boolean;
}

const ProgressBar: React.FC<ProgressBarProps> = ({
  progress,
  className = "",
  size = "medium",
  showSpeed = false,
}) => {
  const sizeClasses = {
    medium: "w-20 h-1.5",
    large: "w-24 h-2",
  };

  const progressClasses = sizeClasses[size];

  if (progress.length === 0) {
    return null;
  }

  if (progress.length === 1) {
    // Single progress bar
    const item = progress[0];
    const percentage = Math.max(0, Math.min(100, item.percentage));

    return (
      <div className={`flex items-center gap-3 ${className}`}>
        <progress
          value={percentage}
          max={100}
          className={`${progressClasses} [&::-webkit-progress-bar]:rounded-full [&::-webkit-progress-bar]:bg-line [&::-webkit-progress-value]:rounded-full [&::-webkit-progress-value]:bg-logo-primary`}
        />
        {showSpeed && (
          <div className="text-xs text-ink-2 tabular-nums min-w-fit">
            {item.speed !== undefined && item.speed > 0 ? (
              // eslint-disable-next-line i18next/no-literal-string
              <span>{item.speed.toFixed(1)}MB/s</span>
            ) : (
              <span>Downloading...</span>
            )}
          </div>
        )}
      </div>
    );
  }

  // Multiple progress bars
  return (
    <div className={`flex items-center gap-2 ${className}`}>
      <div className="flex gap-1">
        {progress.map((item) => {
          const percentage = Math.max(0, Math.min(100, item.percentage));
          return (
            <progress
              key={item.id}
              value={percentage}
              max={100}
              title={item.label || `${percentage}%`}
              className="w-3 h-1.5 [&::-webkit-progress-bar]:rounded-full [&::-webkit-progress-bar]:bg-line [&::-webkit-progress-value]:rounded-full [&::-webkit-progress-value]:bg-logo-primary"
            />
          );
        })}
      </div>
      <div className="text-xs text-ink-2 min-w-fit">
        {progress.length} downloading...
      </div>
    </div>
  );
};

export default ProgressBar;
