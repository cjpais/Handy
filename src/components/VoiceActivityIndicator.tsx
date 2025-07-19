import React, { useState, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';

interface VoiceActivityData {
  level: number;
}

interface RecordingStateData {
  is_recording: boolean;
  binding_id: string;
}

export const VoiceActivityIndicator: React.FC = () => {
  const [isRecording, setIsRecording] = useState(false);
  const [voiceLevel, setVoiceLevel] = useState(0);
  const [bindingId, setBindingId] = useState<string>('');

  // Animation states
  const [pulseIntensity, setPulseIntensity] = useState(0);
  const animationRef = useRef<number>();

  // Voice activity history for smoother visualization
  const voiceHistoryRef = useRef<number[]>([]);

  useEffect(() => {
    // Listen for recording state changes
    const unlistenRecording = listen<RecordingStateData>('recording-state-changed', (event) => {
      setIsRecording(event.payload.is_recording);
      setBindingId(event.payload.binding_id);

      if (!event.payload.is_recording) {
        // Reset levels when recording stops
        setVoiceLevel(0);
        voiceHistoryRef.current = [];
      }
    });

    // Listen for voice activity levels
    const unlistenVoiceActivity = listen<VoiceActivityData>('voice-activity-level', (event) => {
      const level = event.payload.level;

      // Smooth the voice level using a moving average
      voiceHistoryRef.current.push(level);
      if (voiceHistoryRef.current.length > 5) {
        voiceHistoryRef.current.shift();
      }

      const smoothedLevel = voiceHistoryRef.current.reduce((a, b) => a + b, 0) / voiceHistoryRef.current.length;
      setVoiceLevel(smoothedLevel);
    });

    return () => {
      unlistenRecording.then(fn => fn());
      unlistenVoiceActivity.then(fn => fn());
    };
  }, []);

  // Pulse animation based on voice activity
  useEffect(() => {
    if (isRecording) {
      const animate = () => {
        setPulseIntensity(prev => {
          const target = voiceLevel * 0.5 + 0.5; // Convert to 0.5-1.0 range
          return prev + (target - prev) * 0.3; // Smooth transition
        });
        animationRef.current = requestAnimationFrame(animate);
      };
      animationRef.current = requestAnimationFrame(animate);
    } else {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
      }
      setPulseIntensity(0);
    }

    return () => {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
      }
    };
  }, [isRecording, voiceLevel]);

  if (!isRecording) return null;

  return (
    <div className="fixed inset-0 pointer-events-none z-50">
      <div className="absolute bottom-6 left-1/2 transform -translate-x-1/2 pointer-events-auto">
        <div
          className="bg-background/95 backdrop-blur-sm border border-logo-primary/30 rounded-full px-4 py-2 shadow-xl"
          style={{
            transform: `scale(${0.95 + pulseIntensity * 0.05})`,
            transition: 'transform 0.1s ease-out'
          }}
        >
          <div className="flex items-center space-x-3">
            {/* Microphone Icon with Pulse */}
            <div className="relative">
              <div
                className="absolute inset-0 bg-logo-primary rounded-full animate-ping"
                style={{
                  opacity: pulseIntensity * 0.4,
                  animationDuration: `${1.5 - voiceLevel}s`
                }}
              />
              <div
                className="relative w-6 h-6 bg-logo-primary rounded-full flex items-center justify-center"
                style={{
                  backgroundColor: `hsl(${320 + voiceLevel * 40}, 70%, ${60 + voiceLevel * 20}%)`
                }}
              >
                <svg className="w-3 h-3 fill-white" viewBox="0 0 24 24">
                  <path d="M12 2c1.1 0 2 .9 2 2v6c0 1.1-.9 2-2 2s-2-.9-2-2V4c0-1.1.9-2 2-2zm6 6c0 2.76-2.24 5-5 5s-5-2.24-5-5H6c0 3.53 2.61 6.43 6 6.92V21h4v-2.08c3.39-.49 6-3.39 6-6.92h-2z"/>
                </svg>
              </div>
            </div>

            {/* Voice Level Bars */}
            <div className="flex items-end space-x-0.5">
              {[...Array(4)].map((_, i) => {
                const barHeight = Math.max(0.2, Math.min(1, (voiceLevel - i * 0.2) * 2));
                return (
                  <div
                    key={i}
                    className="w-1 bg-logo-primary rounded-full transition-all duration-75"
                    style={{
                      height: `${6 + barHeight * 12}px`,
                      opacity: voiceLevel > i * 0.2 ? 1 : 0.3
                    }}
                  />
                );
              })}
            </div>

            {/* Status Text */}
            <div className="text-xs text-mid-gray font-medium">
              Listening...
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
