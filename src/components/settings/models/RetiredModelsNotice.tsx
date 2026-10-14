import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Loader2, Trash2, TriangleAlert } from "lucide-react";
import { Button } from "../../ui/Button";
import { commands, type RetiredModel } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import { deleteRetiredModel } from "@/components/onboarding/retiredModels";
import { formatModelSize } from "@/lib/utils/format";

/**
 * Offers a one-click cleanup of retired (ONNX) models that are still on disk
 * but can no longer run. The active model is left out: deleting it would leave
 * the user with no model, so that case belongs to the full-screen update flow.
 */
const RetiredModelsNotice: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel } = useModelStore();
  const [onDisk, setOnDisk] = useState<RetiredModel[]>([]);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    commands.getRetiredModels().then(setOnDisk);
  }, []);

  const retired = onDisk.filter((m) => m.id !== currentModel);

  if (retired.length === 0) return null;

  const size = formatModelSize(
    retired.reduce((sum, m) => sum + Number(m.size_mb), 0),
  );

  const handleDelete = async () => {
    setDeleting(true);
    let deleted = 0;
    for (const model of retired) {
      if (await deleteRetiredModel(model.id)) deleted += 1;
    }
    setOnDisk(await commands.getRetiredModels());
    setDeleting(false);

    if (deleted === retired.length) {
      toast(t("settings.models.retiredNotice.deleted"));
    } else {
      toast.error(t("settings.models.retiredNotice.deleteFailed"));
    }
  };

  return (
    <div
      role="status"
      className="flex items-center gap-3 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2.5"
    >
      <TriangleAlert className="h-5 w-5 shrink-0 text-warning" />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium leading-5">
          {t("settings.models.retiredNotice.title")}
        </p>
        <p className="text-sm leading-5 text-text/70">
          {t("settings.models.retiredNotice.description", { size })}
        </p>
      </div>
      <Button
        variant="warning"
        size="md"
        onClick={handleDelete}
        disabled={deleting}
        className="shrink-0 flex items-center gap-1.5"
      >
        {deleting ? (
          <Loader2 className="w-3.5 h-3.5 animate-spin" />
        ) : (
          <Trash2 className="w-3.5 h-3.5" />
        )}
        {t("settings.models.retiredNotice.delete")}
      </Button>
    </div>
  );
};

export default RetiredModelsNotice;
