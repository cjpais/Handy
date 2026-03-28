import React, { useState, useRef, useEffect, useCallback } from "react";
import { Play, Pause } from "lucide-react";

// Global registry: when one player starts, stop all others
const activePlayers = new Set<() => void>();

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
  const [loadError, setLoadError] = useState<string | null>(null);

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const src = loadedSrc;
  const dragTimeRef = useRef<number>(0);

  // Create audio element imperatively (not in JSX) so ref is always stable
  // and event listeners attach before src is set.
  useEffect(() => {
    if (!src) return;

    const audio = new Audio();
    audio.preload = "metadata";
    audioRef.current = audio;

    const handleLoadedMetadata = () => {
      setDuration(audio.duration || 0);
      setCurrentTime(0);
      setLoadError(null);
    };

    const handleDurationChange = () => {
      if (audio.duration && isFinite(audio.duration)) {
        setDuration(audio.duration);
      }
    };

    const handleCanPlay = () => {
      if (audio.duration && isFinite(audio.duration)) {
        setDuration(audio.duration);
      }
      setLoadError(null);
    };

    const handleEnded = () => {
      setIsPlaying(false);
      setCurrentTime(audio.duration || 0);
    };

    const handlePlay = () => setIsPlaying(true);
    const handlePause = () => setIsPlaying(false);

    const handleTimeUpdate = () => {
      setCurrentTime(audio.currentTime);
    };

    const handleError = () => {
      console.error("Audio load error:", audio.error);
      setLoadError(audio.error?.message || "Failed to load audio");
    };

    audio.addEventListener("loadedmetadata", handleLoadedMetadata);
    audio.addEventListener("durationchange", handleDurationChange);
    audio.addEventListener("canplay", handleCanPlay);
    audio.addEventListener("ended", handleEnded);
    audio.addEventListener("play", handlePlay);
    audio.addEventListener("pause", handlePause);
    audio.addEventListener("timeupdate", handleTimeUpdate);
    audio.addEventListener("error", handleError);

    // Set src after attaching listeners
    audio.src = src;

    return () => {
      audio.pause();
      audio.removeEventListener("loadedmetadata", handleLoadedMetadata);
      audio.removeEventListener("durationchange", handleDurationChange);
      audio.removeEventListener("canplay", handleCanPlay);
      audio.removeEventListener("ended", handleEnded);
      audio.removeEventListener("play", handlePlay);
      audio.removeEventListener("pause", handlePause);
      audio.removeEventListener("timeupdate", handleTimeUpdate);
      audio.removeEventListener("error", handleError);
      audio.src = "";
      audioRef.current = null;
    };
  }, [src]); // Recreate when src changes

  // Register stop callback for global "only one player at a time"
  const stopPlayback = useCallback(() => {
    audioRef.current?.pause();
  }, []);

  useEffect(() => {
    activePlayers.add(stopPlayback);
    return () => {
      activePlayers.delete(stopPlayback);
    };
  }, [stopPlayback]);

  // Auto-play when src becomes available (via onLoadRequest)
  const prevLoadedSrc = useRef<string | null>(null);
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    if (loadedSrc && !prevLoadedSrc.current && onLoadRequest) {
      // Stop all others first
      activePlayers.forEach((stop) => {
        if (stop !== stopPlayback) stop();
      });
      audio.play().catch((error) => {
        console.error("Auto-play failed:", error);
      });
    } else if (autoPlay && initialSrc && !prevLoadedSrc.current) {
      activePlayers.forEach((stop) => {
        if (stop !== stopPlayback) stop();
      });
      audio.play().catch((error) => {
        console.error("Auto-play failed:", error);
      });
    }

    prevLoadedSrc.current = loadedSrc;
  }, [loadedSrc, autoPlay, initialSrc, onLoadRequest, stopPlayback]);

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
    if (isLoading) return;

    try {
      if (isPlaying) {
        audio?.pause();
      } else if (!src && onLoadRequest) {
        // If no src loaded yet, request it
        setIsLoading(true);
        const newSrc = await onLoadRequest();
        setIsLoading(false);
        if (newSrc) {
          setLoadedSrc(newSrc);
          // Playback will be triggered by the useEffect watching loadedSrc
        }
      } else if (audio && src) {
        // Stop all other players first
        activePlayers.forEach((stop) => {
          if (stop !== stopPlayback) stop();
        });
        await audio.play();
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

  const getProgressPercent = (): number => {
    if (duration <= 0) return 0;
    if (duration - currentTime < 0.1) return 100;
    const percent = (currentTime / duration) * 100;
    return Math.min(100, Math.max(0, percent));
  };

  const progressPercent = getProgressPercent();

  return (
    <div className={`flex items-center gap-3 ${className}`}>
      {/* No JSX <audio> — created imperatively above */}

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
            background: `linear-gradient(to right, #FAA2CA 0%, #FAA2CA ${progressPercent}%, rgba(128, 128, 128, 0.2) ${progressPercent}%, rgba(128, 128, 128, 0.2) 100%)`,
          }}
        />

        <span className="text-xs text-text/60 min-w-[30px] tabular-nums">
          {formatTime(duration)}
        </span>
      </div>

      {loadError && (
        <div className="text-xs text-red-500 mt-1">Error: {loadError}</div>
      )}
    </div>
  );
};
