import React, { useState, useRef, useEffect, useCallback } from "react";
import { Play, Pause } from "lucide-react";

interface AudioPlayerProps {
  /** Audio source URL. If not provided, onLoadRequest must be provided. */
  src?: string;
  /** Called when play is clicked and no src is loaded yet. Should return the audio URL. */
  onLoadRequest?: () => Promise<string | null>;
  className?: string;
  autoPlay?: boolean;
}

export const AudioPlayer: React.FC<AudioPlayerProps> = ({
  src: initialSrc,
  onLoadRequest,
  className = "",
  autoPlay = false,
}) => {
  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [isDragging, setIsDragging] = useState(false);
  const [loadedSrc, setLoadedSrc] = useState<string | null>(initialSrc ?? null);
  const [isLoading, setIsLoading] = useState(false);

  const audioRef = useRef<HTMLAudioElement>(null);
  const src = loadedSrc;
  const animationRef = useRef<number>();
  const dragTimeRef = useRef<number>(0);

  // Use refs to avoid stale closures in animation loop
  const isPlayingRef = useRef(false);
  const isDraggingRef = useRef(false);

  // Keep refs in sync with state
  useEffect(() => {
    isPlayingRef.current = isPlaying;
  }, [isPlaying]);

  useEffect(() => {
    isDraggingRef.current = isDragging;
  }, [isDragging]);

  // Stable animation loop with no dependencies
  const tick = useCallback(() => {
    if (audioRef.current && !isDraggingRef.current) {
      const time = audioRef.current.currentTime;
      setCurrentTime(time);
    }

    if (isPlayingRef.current) {
      animationRef.current = requestAnimationFrame(tick);
    }
  }, []); // Empty dependency array is key!

  // Manage animation loop lifecycle
  useEffect(() => {
    if (isPlaying && !isDragging) {
      // Only start if not already running
      if (!animationRef.current) {
        animationRef.current = requestAnimationFrame(tick);
      }
    } else {
      // Stop animation loop
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
        animationRef.current = undefined;
      }
    }

    return () => {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
        animationRef.current = undefined;
      }
    };
  }, [isPlaying, isDragging, tick]);

  // Audio event handlers
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    const handleLoadedMetadata = () => {
      setDuration(audio.duration || 0);
      setCurrentTime(0);
    };

    const handleEnded = () => {
      setIsPlaying(false);
      setCurrentTime(audio.duration || 0);
    };

    const handlePlay = () => setIsPlaying(true);
    const handlePause = () => setIsPlaying(false);

    audio.addEventListener("loadedmetadata", handleLoadedMetadata);
    audio.addEventListener("ended", handleEnded);
    audio.addEventListener("play", handlePlay);
    audio.addEventListener("pause", handlePause);

    return () => {
      audio.removeEventListener("loadedmetadata", handleLoadedMetadata);
      audio.removeEventListener("ended", handleEnded);
      audio.removeEventListener("play", handlePlay);
      audio.removeEventListener("pause", handlePause);
    };
  }, []);

  // Auto-play when src becomes available (via onLoadRequest or autoPlay prop)
  const prevLoadedSrc = useRef<string | null>(null);
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    // Play when loadedSrc changes from null to a value (lazy load case)
    if (loadedSrc && !prevLoadedSrc.current && onLoadRequest) {
      audio.play().catch((error) => {
        console.error("Auto-play failed:", error);
      });
    }
    // Or when autoPlay is set with initial src
    else if (autoPlay && initialSrc && !prevLoadedSrc.current) {
      audio.play().catch((error) => {
        console.error("Auto-play failed:", error);
      });
    }

    prevLoadedSrc.current = loadedSrc;
  }, [loadedSrc, autoPlay, initialSrc, onLoadRequest]);

  // Global drag handlers
  const handleMouseUp = useCallback(() => {
    if (isDragging) {
      setIsDragging(false);
      if (audioRef.current) {
        audioRef.current.currentTime = dragTimeRef.current;
        setCurrentTime(dragTimeRef.current);
      }
    }
  }, [isDragging]);

  useEffect(() => {
    if (isDragging) {
      document.addEventListener("mouseup", handleMouseUp);
      document.addEventListener("touchend", handleMouseUp);

      return () => {
        document.removeEventListener("mouseup", handleMouseUp);
        document.removeEventListener("touchend", handleMouseUp);
      };
    }
  }, [isDragging, handleMouseUp]);

  // Cleanup blob URLs on unmount
  useEffect(() => {
    return () => {
      if (loadedSrc?.startsWith("blob:")) {
        URL.revokeObjectURL(loadedSrc);
      }
    };
  }, [loadedSrc]);

  const togglePlay = async () => {
    const audio = audioRef.current;
    if (!audio) return;
    if (isLoading) return;

    try {
      if (isPlaying) {
        audio.pause();
      } else {
        // If no src loaded yet, request it
        if (!src && onLoadRequest) {
          setIsLoading(true);
          const newSrc = await onLoadRequest();
          setIsLoading(false);
          if (newSrc) {
            setLoadedSrc(newSrc);
            // Playback will be triggered by the useEffect watching loadedSrc
          }
        } else if (src) {
          await audio.play();
        }
      }
    } catch (error) {
      console.error("Playback failed:", error);
    }
  };

  const handleSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newTime = parseFloat(e.target.value);
    dragTimeRef.current = newTime;
    setCurrentTime(newTime);

    if (!isDragging && audioRef.current) {
      audioRef.current.currentTime = newTime;
    }
  };

  const handleSliderMouseDown = () => {
    setIsDragging(true);
  };

  const handleSliderTouchStart = () => {
    setIsDragging(true);
  };

  const formatTime = (time: number): string => {
    if (!isFinite(time)) return "0:00";

    const minutes = Math.floor(time / 60);
    const seconds = Math.floor(time % 60);
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  };

  // Fix playhead positioning with better edge case handling
  const getProgressPercent = (): number => {
    if (duration <= 0) return 0;

    // Handle the end case - if we're within 0.1 seconds of the end, show 100%
    if (duration - currentTime < 0.1) return 100;

    const percent = (currentTime / duration) * 100;
    return Math.min(100, Math.max(0, percent));
  };

  const progressPercent = getProgressPercent();

  return (
    <div className={`flex items-center gap-3 ${className}`}>
      <audio ref={audioRef} src={src ?? undefined} preload="metadata" />

      <button
        onClick={togglePlay}
        disabled={isLoading}
        className="transition-colors cursor-pointer text-text hover:text-logo-primary disabled:opacity-50"
        aria-label={isPlaying ? "Pause" : "Play"}
      >
        {isPlaying ? (
          <Pause width={20} height={20} fill="currentColor" />
        ) : (
          <Play width={20} height={20} fill="currentColor" />
        )}
      </button>

      <div className="flex-1 flex items-center gap-2">
        <span className="text-xs text-text/60 min-w-[30px] tabular-nums">
          {formatTime(currentTime)}
        </span>

        <input
          type="range"
          min="0"
          max={duration || 0}
          step="0.01"
          value={currentTime}
          onChange={handleSeek}
          onMouseDown={handleSliderMouseDown}
          onTouchStart={handleSliderTouchStart}
          className={`flex-1 h-1 rounded-lg appearance-none cursor-pointer focus:outline-none focus:ring-1 focus:ring-logo-primary ${progressPercent >= 99.5 ? "[&::-webkit-slider-thumb]:translate-x-0.5 [&::-moz-range-thumb]:translate-x-0.5" : ""}`}
          style={{
            background: `linear-gradient(to right, var(--color-background-ui) 0%, var(--color-background-ui) ${progressPercent}%, rgba(128, 128, 128, 0.2) ${progressPercent}%, rgba(128, 128, 128, 0.2) 100%)`,
          }}
        />

        <span className="text-xs text-text/60 min-w-[30px] tabular-nums">
          {formatTime(duration)}
        </span>
      </div>
    </div>
  );
};
