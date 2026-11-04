import { useEffect, useState } from "react";
import { commands } from "@/bindings";
import type { MachineTier } from "@/lib/models/catalog";
import { useSettings } from "./useSettings";

/** Benchmark tier: "accelerated" when there's a GPU and CPU isn't forced. */
export const useMachineTier = (): MachineTier => {
  const { getSetting } = useSettings();
  const accelerator = getSetting("transcribe_accelerator");
  const [hasGpu, setHasGpu] = useState(true);
  useEffect(() => {
    commands
      .getAvailableAccelerators()
      .then((a) => setHasGpu(a.gpu_devices.length > 0))
      .catch(() => setHasGpu(false));
  }, []);
  return hasGpu && accelerator !== "cpu" ? "accelerated" : "cpu";
};
