import { useEffect, useState } from "react";
import { commands } from "@/bindings";
import { useOsType } from "./useOsType";

/**
 * Detect if the app is running under Wayland (Linux only).
 */
export function useWayland(): boolean {
  const osType = useOsType();
  const [isWayland, setIsWayland] = useState(false);

  useEffect(() => {
    let isMounted = true;

    if (osType !== "linux") {
      setIsWayland(false);
      return;
    }

    commands
      .isWaylandActive()
      .then((result) => {
        if (!isMounted) return;
        setIsWayland(result);
      })
      .catch(() => {
        if (isMounted) setIsWayland(false);
      });

    return () => {
      isMounted = false;
    };
  }, [osType]);

  return isWayland;
}
