import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "../../stores/settingsStore";
import { commands } from "@/bindings";
import { toast } from "sonner";
import { relaunch } from "@tauri-apps/plugin-process";

interface ProxySettingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const DEFAULT_PROXY_PLACEHOLDER = "http://127.0.0.1:7890";

export const ProxySetting: React.FC<ProxySettingProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();

  const storedProxy = getSetting("proxy_url") ?? null;
  const [enabled, setEnabled] = useState<boolean>(!!storedProxy);
  const [draft, setDraft] = useState<string>(storedProxy ?? "");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setEnabled(!!storedProxy);
    setDraft(storedProxy ?? "");
  }, [storedProxy]);

  const normalizedDraft = enabled ? draft.trim() : null;
  const dirty = (normalizedDraft || null) !== (storedProxy || null);

  const handleSave = async () => {
    const target = enabled ? draft.trim() : "";
    if (enabled) {
      if (!target) {
        toast.error(
          t("settings.advanced.proxy.errors.empty", {
            defaultValue: "代理地址不能为空",
          }),
        );
        return;
      }
      try {
        const u = new URL(target);
        if (u.protocol !== "http:" && u.protocol !== "socks5:") {
          throw new Error("unsupported");
        }
      } catch {
        toast.error(
          t("settings.advanced.proxy.errors.invalid", {
            defaultValue:
              "代理地址无效,需为 http:// 或 socks5:// 开头的完整 URL",
          }),
        );
        return;
      }
    }

    const payload = target ? target : null;
    setSaving(true);
    const result = await commands.changeProxyUrlSetting(payload);
    setSaving(false);
    if (result.status === "error") {
      const raw = result.error ?? "";
      if (raw.startsWith("PROXY_UNREACHABLE:")) {
        const addr = raw.slice("PROXY_UNREACHABLE:".length);
        toast.error(
          t("settings.advanced.proxy.errors.unreachable", {
            defaultValue: `代理 ${addr} 无法连接,请确认代理服务正在运行`,
            addr,
          }),
        );
      } else if (raw.startsWith("PROXY_URL_INVALID:")) {
        toast.error(
          t("settings.advanced.proxy.errors.invalid", {
            defaultValue:
              "代理地址无效,需为 http:// 或 socks5:// 开头的完整 URL",
          }),
        );
      } else {
        toast.error(raw || t("common.error", { defaultValue: "保存失败" }));
      }
      return;
    }

    const { settings, setSettings } = useSettingsStore.getState();
    if (settings) {
      setSettings({ ...settings, proxy_url: payload });
    }

    toast.success(
      t("settings.advanced.proxy.savedRestart", {
        defaultValue: "已保存,重启应用后代理生效",
      }),
      {
        action: {
          label: t("settings.advanced.proxy.restartNow", {
            defaultValue: "立即重启",
          }),
          onClick: () => {
            void relaunch();
          },
        },
        duration: 8000,
      },
    );
  };

  return (
    <SettingContainer
      title={t("settings.advanced.proxy.title", { defaultValue: "网页代理" })}
      description={t("settings.advanced.proxy.description", {
        defaultValue:
          "为内嵌 AI 对话网页配置 HTTP 或 SOCKS5 代理,用于访问被墙站点(例如 ChatGPT)。修改后需要重启应用生效。",
      })}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex flex-col gap-2">
        <label className="inline-flex items-center gap-2 text-sm select-none cursor-pointer w-fit">
          <input
            type="checkbox"
            className="accent-logo-primary"
            checked={enabled}
            onChange={(e) => {
              const next = e.target.checked;
              setEnabled(next);
              if (!next) setDraft("");
            }}
          />
          <span>
            {t("settings.advanced.proxy.enable", { defaultValue: "启用代理" })}
          </span>
        </label>
        <div className="flex items-center gap-2">
          <Input
            type="text"
            className="flex-1"
            placeholder={DEFAULT_PROXY_PLACEHOLDER}
            value={draft}
            disabled={!enabled || saving}
            onChange={(e) => setDraft(e.target.value)}
            spellCheck={false}
            autoComplete="off"
          />
          <Button
            variant="primary"
            onClick={handleSave}
            disabled={saving || !dirty}
          >
            {saving
              ? t("common.saving", { defaultValue: "保存中…" })
              : t("common.save", { defaultValue: "保存" })}
          </Button>
        </div>
      </div>
    </SettingContainer>
  );
};
