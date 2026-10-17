import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import type { HandyImportOffer } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { useSettings } from "@/hooks/useSettings";

/**
 * First-run offer to copy custom words from an installed Handy. Shows nothing
 * when Handy is absent, has nothing new, or the offer was answered before. The
 * Handy file is only read.
 */
export const HandyImportBanner: React.FC = () => {
  const { t } = useTranslation();
  const { refreshSettings } = useSettings();
  const [offer, setOffer] = useState<HandyImportOffer | null>(null);

  useEffect(() => {
    let cancelled = false;
    commands
      .voxaGetHandyImport()
      .then((found) => {
        if (!cancelled) setOffer(found);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  if (!offer) return null;

  const importWords = async () => {
    const result = await commands.voxaImportHandyWords();
    if (result.status === "ok") {
      toast.success(t("voxa.settings.handyImport.imported"));
      await refreshSettings();
    } else {
      toast.error(result.error);
    }
    setOffer(null);
  };

  const dismiss = async () => {
    await commands.voxaDismissHandyImport();
    setOffer(null);
  };

  return (
    <section className="vg-card px-4 py-3 flex flex-col gap-2" role="region">
      <h3 className="vg-card-title">{t("voxa.settings.handyImport.title")}</h3>
      <p className="vg-muted">
        {t("voxa.settings.handyImport.body", {
          words: offer.new_words.slice(0, 8).join(", "),
        })}
      </p>
      <div className="flex gap-2 justify-end">
        <Button variant="ghost" size="sm" onClick={() => void dismiss()}>
          {t("voxa.settings.handyImport.dismiss")}
        </Button>
        <Button variant="primary" size="sm" onClick={() => void importWords()}>
          {t("voxa.settings.handyImport.import")}
        </Button>
      </div>
    </section>
  );
};

export default HandyImportBanner;
