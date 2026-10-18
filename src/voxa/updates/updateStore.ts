import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { commands, type UpdateStatus } from "@/bindings";

/** Event the backend emits on every update state change. */
export const UPDATE_STATUS_EVENT = "voxa-update-status";
/** Tray menu and footer ask the window to open the page with the update card. */
export const OPEN_UPDATES_EVENT = "voxa-open-updates";

interface UpdateStore {
  status: UpdateStatus | null;
  /** Loads the current state and follows backend changes. Returns the unsubscribe. */
  init: () => Promise<() => void>;
  check: () => Promise<void>;
  install: () => Promise<void>;
}

export const useUpdateStore = create<UpdateStore>((set) => ({
  status: null,

  init: async () => {
    const unlisten = await listen<UpdateStatus>(UPDATE_STATUS_EVENT, (e) =>
      set({ status: e.payload }),
    );
    const current = await commands.voxaUpdateStatus();
    if (current.status === "ok") set({ status: current.data });
    return unlisten;
  },

  check: async () => {
    const result = await commands.voxaCheckForUpdates();
    if (result.status === "ok") set({ status: result.data });
  },

  install: async () => {
    // Returns only when the install failed; the backend has published the error.
    const result = await commands.voxaInstallUpdate();
    if (result.status === "error") {
      const current = await commands.voxaUpdateStatus();
      if (current.status === "ok") set({ status: current.data });
    }
  },
}));

/** `no_release` -> `noRelease`, for translation keys. */
export function errorKey(kind: string): string {
  return kind.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

/** The announced version, only while it waits for the owner's click. */
export function availableVersion(status: UpdateStatus | null): string | null {
  return status?.phase === "available" ? status.version : null;
}
