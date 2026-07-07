import React, { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

interface LiveWaveformProps {
  active: boolean;
  processing: boolean;
  height?: number;
  barWidth?: number;
  barGap?: number;
  mode?: "static" | "scrolling";
  fadeEdges?: boolean;
  barColor?: string;
  historySize?: number;
}

export function LiveWaveform({
  active,
  processing,
  height = 80,
  barWidth = 3,
  barGap = 2,
  mode = "static",
  fadeEdges = true,
  barColor = "gray",
  historySize = 120,
}: LiveWaveformProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const levelsRef = useRef<number[]>([]);
  const phaseRef = useRef<number>(0);
  const currentHeightsRef = useRef<number[]>([]);
  const hasReceivedEventsRef = useRef<boolean>(false);

  // Listen to Tauri mic-levels when active (updates Ref, no React state change!)
  useEffect(() => {
    if (!active) {
      levelsRef.current = [];
      hasReceivedEventsRef.current = false;
      return;
    }

    let unlisten: (() => void) | null = null;
    let smoothedLevels = Array(9).fill(0);

    const setupListener = async () => {
      try {
        unlisten = await listen<number[]>("mic-level", (event) => {
          hasReceivedEventsRef.current = true;
          const newLevels = event.payload || [];
          if (newLevels.length === 0) return;

          // Squelch Noise Gate to cut out background mic static hum
          const maxVal = Math.max(...newLevels);
          const noiseGateThreshold = 0.02;
          
          let targetLevels = newLevels;
          if (maxVal < noiseGateThreshold) {
            targetLevels = Array(newLevels.length).fill(0);
          }

          // Smooth incoming levels
          const smoothed = smoothedLevels.map((prev, i) => {
            const target = targetLevels[i] || 0;
            // Decay to silence faster for instant stabilization
            const decayFactor = target === 0 ? 0.45 : 0.75;
            return prev * decayFactor + target * (1 - decayFactor);
          });
          smoothedLevels = smoothed;
          levelsRef.current = smoothed;
        });
      } catch (err) {
        console.warn("Failed to listen to Tauri mic-level event:", err);
      }
    };

    setupListener();

    return () => {
      if (unlisten) unlisten();
    };
  }, [active]);

  // Browser preview mode: simulated mic fallback (only when NOT in Tauri)
  useEffect(() => {
    if (!active) return;

    const isTauri = typeof window !== "undefined" && ((window as any).__TAURI_INTERNALS__ !== undefined || (window as any).__TAURI__ !== undefined);
    if (isTauri) return;

    const fallbackInterval = setInterval(() => {
      if (!hasReceivedEventsRef.current) {
        const simulatedLevels = Array(9)
          .fill(0)
          .map(() => Math.random() * 0.35 + (Math.sin(Date.now() / 200) * 0.05));
        levelsRef.current = simulatedLevels;
      }
    }, 100);

    return () => {
      clearInterval(fallbackInterval);
    };
  }, [active]);

  // Direct DOM Mutating Render Loop (GPU & CPU friendly)
  useEffect(() => {
    let animationFrameId: number;
    const totalBars = 32;
    const minHeight = 4;

    // Initialize smoothing height array
    if (currentHeightsRef.current.length !== totalBars) {
      currentHeightsRef.current = Array(totalBars).fill(minHeight);
    }

    const render = () => {
      const container = containerRef.current;
      if (!container) {
        animationFrameId = requestAnimationFrame(render);
        return;
      }

      const bars = container.children;
      if (bars.length !== totalBars) {
        animationFrameId = requestAnimationFrame(render);
        return;
      }

      const levels = levelsRef.current;
      
      // Determine colors dynamically
      let resolvedBgColor = barColor;
      try {
        if (barColor.startsWith("var(")) {
          const varName = barColor.slice(4, -1);
          resolvedBgColor = getComputedStyle(container).getPropertyValue(varName).trim() || resolvedBgColor;
        } else if (barColor === "gray") {
          resolvedBgColor = getComputedStyle(container).getPropertyValue("--color-pebble").trim() || "var(--color-pebble)";
        }
      } catch (e) {}

      // Increment phase
      phaseRef.current = (phaseRef.current + 0.018) % (Math.PI * 2);

      // Back-and-forth sloshing phase for left-to-right-and-back wave
      const sloshPhase = Math.sin(phaseRef.current) * Math.PI * 1.3;

      for (let i = 0; i < totalBars; i++) {
        let targetHeight = minHeight;

        if (processing) {
          // Left-to-right-and-back sloshing wave
          const waveVal = Math.sin(i * 0.22 + sloshPhase);
          const val = (waveVal + 1) / 2; // scale to 0..1
          targetHeight = minHeight + val * (height - minHeight) * 0.75;
        } else if (active) {
          // Symmetrical mic level visualization
          const distanceFromCenter = Math.abs(i - (totalBars - 1) / 2);
          const normalizedDist = distanceFromCenter / (totalBars / 2);

          if (levels.length > 0) {
            // Map the center of the visualizer (normalizedDist near 0) to high-energy/low-frequency bands (index 0)
            const levelIndex = Math.min(
              levels.length - 1,
              Math.floor(normalizedDist * levels.length)
            );
            const rawVal = levels[levelIndex] || 0;
            const val = rawVal < 0.015 ? 0 : rawVal;
            // Taper the height: center has maximum scaling (1.0), tapering down to 0 at the edges
            const taper = 1 - normalizedDist;
            const finalVal = Math.max(0, Math.pow(val, 0.75)) * taper;
            targetHeight = minHeight + finalVal * (height - minHeight);
          }
        }

        // Apply a visual damping to keep height changes organic and smooth
        const currentHeight = currentHeightsRef.current[i] || minHeight;
        const newHeight = currentHeight * 0.72 + targetHeight * 0.28;
        currentHeightsRef.current[i] = newHeight;

        // Direct style mutation (bypasses React reconciliation for absolute efficiency)
        const bar = bars[i] as HTMLDivElement;
        if (bar) {
          bar.style.height = `${newHeight}px`;
          if (resolvedBgColor && bar.style.backgroundColor !== resolvedBgColor) {
            bar.style.backgroundColor = resolvedBgColor;
          }
        }
      }

      animationFrameId = requestAnimationFrame(render);
    };

    render();

    return () => {
      cancelAnimationFrame(animationFrameId);
    };
  }, [active, processing, height, barColor]);

  // Initial layout markup (divs are initialized flat)
  const totalBars = 32;
  const minHeight = 4;

  const renderInitialBars = () => {
    const bars = [];
    for (let i = 0; i < totalBars; i++) {
      let resolvedBgColor = barColor;
      if (barColor.startsWith("var(")) {
        resolvedBgColor = barColor;
      } else if (barColor === "gray") {
        resolvedBgColor = "var(--color-pebble)";
      }

      bars.push(
        <div
          key={i}
          className="rounded-full"
          style={{
            width: `${barWidth}px`,
            height: `${minHeight}px`,
            marginRight: i === totalBars - 1 ? 0 : `${barGap}px`,
            backgroundColor: resolvedBgColor,
          }}
        />
      );
    }
    return bars;
  };

  return (
    <div
      className="w-full relative overflow-hidden flex items-center justify-center"
      style={{
        height: `${height}px`,
        maskImage: fadeEdges
          ? "linear-gradient(to right, transparent, white 15%, white 85%, transparent)"
          : undefined,
        WebkitMaskImage: fadeEdges
          ? "linear-gradient(to right, transparent, white 15%, white 85%, transparent)"
          : undefined,
      }}
    >
      <div ref={containerRef} className="flex items-center justify-center h-full">
        {renderInitialBars()}
      </div>
    </div>
  );
}
