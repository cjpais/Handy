import React, { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

interface AudioVisualizerProps {
  isRecording: boolean;
  barCount?: number;
}

export const AudioVisualizer: React.FC<AudioVisualizerProps> = ({
  isRecording,
  barCount = 9,
}) => {
  const [levels, setLevels] = useState<number[]>(Array(barCount).fill(0));

  useEffect(() => {
    if (!isRecording) {
      setLevels(Array(barCount).fill(0));
      return;
    }

    const unlisten = listen<number[]>("mic-level", (event) => {
      // The backend sends 16 bands, we'll use the first barCount
      const rawLevels = event.payload.slice(0, barCount);
      setLevels((prev) =>
        rawLevels.map((target, i) => {
          // Smooth the transition
          return prev[i] * 0.6 + target * 0.4;
        })
      );
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [isRecording, barCount]);

  return (
    <div className="flex items-end justify-center gap-1 h-16 px-4">
      {levels.map((level, index) => (
        <div
          key={index}
          className="w-2 bg-logo-primary rounded-full transition-all duration-75"
          style={{
            height: `${Math.max(4, level * 100)}%`,
            opacity: isRecording ? 1 : 0.3,
          }}
        />
      ))}
    </div>
  );
};
