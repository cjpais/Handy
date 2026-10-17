import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands, events } from "@/bindings";
import type { VoxaStats } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { useSettingsStore } from "@/stores/settingsStore";
import { StatsView } from "./StatsView";

/** Loads the statistics, keeps them fresh and wires the actions. */
export const StatsScreen: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [stats, setStats] = useState<VoxaStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pendingTerm, setPendingTerm] = useState<string | null>(null);
  const refreshSettings = useSettingsStore((s) => s.refreshSettings);
  const requestId = useRef(0);

  const load = useCallback(async () => {
    const id = ++requestId.current;
    try {
      const result = await commands.getVoxaStats();
      if (id !== requestId.current) return;
      if (result.status === "ok") {
        setStats(result.data);
        setError(null);
      } else {
        setError(result.error);
      }
    } catch (e) {
      if (id === requestId.current) setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // A new dictation lands in the statistics just after its history entry, so
  // wait a moment before asking again.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unlisten = events.historyUpdatePayload.listen((event) => {
      if (event.payload.action !== "added") return;
      clearTimeout(timer);
      timer = setTimeout(() => void load(), 700);
    });
    return () => {
      clearTimeout(timer);
      void unlisten.then((fn) => fn());
    };
  }, [load]);

  useEffect(() => {
    const onFocus = () => void load();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [load]);

  const handleAddWord = async (term: string) => {
    setPendingTerm(term);
    try {
      const result = await commands.voxaAddDictionaryWord(term);
      if (result.status === "ok") {
        toast.success(t("voxa.stats.candidates.toastAdded", { term }));
        await Promise.all([refreshSettings(), load()]);
      } else {
        toast.error(
          t("voxa.stats.candidates.toastError", { error: result.error }),
        );
      }
    } catch (e) {
      toast.error(t("voxa.stats.candidates.toastError", { error: String(e) }));
    } finally {
      setPendingTerm(null);
    }
  };

  const handleClear = async () => {
    const confirmed = await ask(t("voxa.stats.footer.clearConfirm"), {
      title: t("voxa.stats.footer.clearTitle"),
      kind: "warning",
    });
    if (!confirmed) return;
    const result = await commands.voxaClearStats();
    if (result.status === "ok") {
      toast.success(t("voxa.stats.footer.cleared"));
      await load();
    } else {
      toast.error(result.error);
    }
  };

  if (error && !stats) {
    return (
      <div className="vg-card vg-empty max-w-3xl w-full mx-auto">
        <p className="vg-muted">{t("voxa.stats.error", { error })}</p>
        <Button variant="secondary" size="sm" onClick={() => void load()}>
          {t("voxa.stats.retry")}
        </Button>
      </div>
    );
  }

  if (!stats) {
    return (
      <div className="vg-stats" aria-busy="true">
        <p className="vg-muted">{t("voxa.stats.loading")}</p>
      </div>
    );
  }

  return (
    <StatsView
      stats={stats}
      locale={i18n.language}
      pendingTerm={pendingTerm}
      onAddWord={handleAddWord}
      onClear={handleClear}
    />
  );
};

export default StatsScreen;
