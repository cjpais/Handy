import { create } from "zustand";
import {
  commands,
  events,
  type Notice,
  type NoticeLevel,
  type NoticeSource,
} from "@/bindings";

/** A message from the frontend, so the backend doesn't hold it. */
export interface LocalNotice {
  id: string;
  level: NoticeLevel;
  title: string;
  detail?: string;
  action?: { label: string; onClick: () => void };
  /** Waits in the footer after its toast, like a backend notice. */
  kept: boolean;
}

export interface NotifyOptions {
  title: string;
  detail?: string;
  level?: NoticeLevel;
  action?: LocalNotice["action"];
  /**
   * Keeps the notice in the footer until it's dismissed or cleared with this
   * key. Without one it's a click's result, and ends with its toast.
   */
  key?: string;
}

// The one place the frontend handles notices. A new one pops up as a toast.
// Dismissing it ends it; otherwise the toast times out and a kept notice waits
// in the footer, which can open the full list.
interface NoticeStore {
  // Mirrors the backend's active notices, newest first.
  notices: Notice[];
  // Newest first.
  local: LocalNotice[];
  // What's popped up right now: a notice's source, or a local notice's id.
  toasts: string[];
  panelOpen: boolean;
  // Decides where the card sits: there's no footer during onboarding.
  footerShown: boolean;
  initialized: boolean;
  initialize: () => Promise<void>;
  notify: (options: NotifyOptions) => void;
  openPanel: () => void;
  /** Takes everything off screen. Kept notices wait in the footer. */
  hide: () => void;
  /** Ends a backend notice: here, in the tray and in the footer. */
  dismiss: (notice: Notice) => void;
  clearLocal: (id: string) => void;
  setFooterShown: (shown: boolean) => void;
}

// Drops toasts whose notice has ended, and closes the panel once it's empty.
const prune = (
  state: Pick<NoticeStore, "notices" | "local" | "toasts" | "panelOpen">,
) => ({
  ...state,
  toasts: state.toasts.filter(
    (id) =>
      state.local.some((notice) => notice.id === id) ||
      state.notices.some((notice) => notice.source === id),
  ),
  panelOpen: state.panelOpen && state.notices.length + state.local.length > 0,
});

export const useNoticeStore = create<NoticeStore>()((set, get) => ({
  notices: [],
  local: [],
  toasts: [],
  panelOpen: false,
  footerShown: false,
  initialized: false,

  initialize: async () => {
    if (get().initialized) return;
    set({ initialized: true });

    // Listen first so nothing is missed. An event then outranks the snapshot,
    // which may have been read before it.
    let changed = false;
    await events.noticesChanged.listen(({ payload: { notices, raised } }) => {
      changed = true;
      set(({ toasts, panelOpen, local }) => {
        // The open panel is already showing it.
        const popped: NoticeSource[] =
          raised && !panelOpen ? [raised.source] : [];
        return prune({
          notices,
          local,
          toasts: [...new Set([...toasts, ...popped])],
          panelOpen,
        });
      });
    });
    const notices = await commands.getActiveNotices();
    if (!changed) set({ notices });
  },

  notify: ({ title, detail, level = "error", action, key }) =>
    set(({ local, toasts, panelOpen }) => {
      // The same failure twice is one row.
      const id = key ?? `${title}|${detail ?? ""}`;
      return {
        local: [
          { id, level, title, detail, action, kept: key !== undefined },
          ...local.filter((notice) => notice.id !== id),
        ],
        toasts: panelOpen ? toasts : [...new Set([...toasts, id])],
      };
    }),

  openPanel: () => set({ panelOpen: true, toasts: [] }),

  hide: () =>
    set(({ local }) => ({
      panelOpen: false,
      toasts: [],
      local: local.filter((notice) => notice.kept),
    })),

  dismiss: (notice) => {
    commands.dismissNotice(notice.source, notice.code);
  },

  clearLocal: (id) =>
    set((state) =>
      prune({
        ...state,
        local: state.local.filter((notice) => notice.id !== id),
      }),
    ),

  setFooterShown: (footerShown) => set({ footerShown }),
}));

/** Shows a message from the frontend. It's the app's only toast. */
export const notify = (options: NotifyOptions) =>
  useNoticeStore.getState().notify(options);
