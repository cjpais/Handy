import React, { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./TargetHighlight.css";

const FLASH_DURATION_MS = 200;

const TargetHighlight: React.FC = () => {
  const [flashing, setFlashing] = useState(false);

  useEffect(() => {
    let unlistenFn: (() => void) | null = null;
    let timeoutId: ReturnType<typeof setTimeout> | null = null;

    listen("target-highlight-flash", () => {
      setFlashing(true);
      if (timeoutId) clearTimeout(timeoutId);
      timeoutId = setTimeout(() => setFlashing(false), FLASH_DURATION_MS);
    }).then((fn) => {
      unlistenFn = fn;
    });

    return () => {
      if (unlistenFn) unlistenFn();
      if (timeoutId) clearTimeout(timeoutId);
    };
  }, []);

  return (
    <div className={`target-highlight-frame${flashing ? " flashing" : ""}`} />
  );
};

export default TargetHighlight;
