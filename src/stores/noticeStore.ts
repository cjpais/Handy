import { create } from "zustand";
import { commands, events, type Notice } from "@/bindings";

// Mirrors the backend's active notices, newest first.
interface NoticeStore {
  notices: Notice[];
  initialized: boolean;
  initialize: () => Promise<void>;
}

export const useNoticeStore = create<NoticeStore>()((set, get) => ({
  notices: [],
  initialized: false,

  initialize: async () => {
    if (get().initialized) return;
    set({ initialized: true });

    // Listen first so nothing is missed. An event then outranks the snapshot,
    // which may have been read before it.
    let changed = false;
    await events.noticesChanged.listen(({ payload }) => {
      changed = true;
      set({ notices: payload.notices });
    });
    const notices = await commands.getActiveNotices();
    if (!changed) set({ notices });
  },
}));
