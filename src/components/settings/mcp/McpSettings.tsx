import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, FlaskConical } from "lucide-react";
import { toast } from "sonner";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { ShortcutInput } from "../ShortcutInput";
import { useSettings } from "../../../hooks/useSettings";
import { commands, type McpServerConfig, type McpToolInfo } from "@/bindings";

const emptyForm: McpServerConfig = {
  id: "",
  name: "",
  command: "",
  args: [],
  env: {},
  enabled: true,
};

export const McpSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();

  const enabled = getSetting("mcp_enabled") || false;
  const autoApproved = (getSetting("mcp_auto_approved_tools") ??
    []) as string[];

  const [servers, setServers] = useState<McpServerConfig[]>([]);
  const [catalog, setCatalog] = useState<McpToolInfo[]>([]);
  const [form, setForm] = useState<McpServerConfig | null>(null);
  const [testing, setTesting] = useState(false);

  const refresh = useCallback(async () => {
    setServers(await commands.getMcpServers());
    setCatalog(await commands.getMcpToolCatalog());
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh, enabled]);

  const saveServer = async () => {
    if (!form) return;
    const config = {
      ...form,
      id: form.id.trim() || form.name.trim().toLowerCase().replace(/\s+/g, "-"),
    };
    const result = await commands.upsertMcpServer(config);
    if (result.status === "error") {
      toast.error(result.error);
      return;
    }
    setForm(null);
    // Give the background sync a moment before refreshing the catalog.
    setTimeout(refresh, 1500);
  };

  const removeServer = async (serverId: string) => {
    await commands.removeMcpServer(serverId);
    await refreshSettings();
    setTimeout(refresh, 500);
  };

  const testServer = async () => {
    if (!form) return;
    setTesting(true);
    const result = await commands.testMcpServer({
      ...form,
      id: form.id.trim() || "test",
    });
    setTesting(false);
    if (result.status === "error") {
      toast.error(t("settings.mcp.form.testFailed"), {
        description: result.error,
      });
    } else {
      toast.success(
        t("settings.mcp.form.testOk", { count: result.data.length }),
        {
          description: result.data.map((tool) => tool.name).join(", "),
        },
      );
    }
  };

  const toggleAutoApprove = async (toolKey: string, approved: boolean) => {
    await commands.setMcpToolAutoApproved(toolKey, approved);
    await refreshSettings();
  };

  return (
    <SettingsGroup title={t("settings.mcp.title")}>
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("mcp_enabled", value)}
        isUpdating={isUpdating("mcp_enabled")}
        label={t("settings.mcp.enabled.label")}
        description={t("settings.mcp.enabled.description")}
        descriptionMode="tooltip"
        grouped
      />
      {enabled && (
        <>
          <ShortcutInput shortcutId="voice_command" grouped />

          <SettingContainer
            title={t("settings.mcp.servers.title")}
            description={t("settings.mcp.servers.description")}
            descriptionMode="tooltip"
            grouped
            layout="stacked"
          >
            <div className="flex flex-col gap-2">
              {servers.map((server) => (
                <div
                  key={server.id}
                  className="flex items-center justify-between rounded-lg bg-muted/60 px-3 py-2 text-sm"
                >
                  <div className="min-w-0">
                    <span className="font-medium">{server.name}</span>{" "}
                    <span className="text-muted-foreground text-xs truncate">
                      {server.command} {(server.args ?? []).join(" ")}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setForm(server)}
                    >
                      {t("settings.mcp.servers.edit")}
                    </Button>
                    <Button
                      variant="danger-ghost"
                      size="sm"
                      onClick={() => removeServer(server.id)}
                      aria-label={t("settings.mcp.servers.remove")}
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </Button>
                  </div>
                </div>
              ))}
              {!form && (
                <div>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => setForm(emptyForm)}
                  >
                    <span className="inline-flex items-center gap-1.5">
                      <Plus className="w-3.5 h-3.5" />
                      {t("settings.mcp.servers.add")}
                    </span>
                  </Button>
                </div>
              )}
              {form && (
                <div className="flex flex-col gap-2 rounded-lg border border-border/60 p-3">
                  <Input
                    placeholder={t("settings.mcp.form.name")}
                    value={form.name}
                    onChange={(e) => setForm({ ...form, name: e.target.value })}
                  />
                  <Input
                    placeholder={t("settings.mcp.form.command")}
                    value={form.command}
                    onChange={(e) =>
                      setForm({ ...form, command: e.target.value })
                    }
                  />
                  <Input
                    placeholder={t("settings.mcp.form.args")}
                    value={(form.args ?? []).join(" ")}
                    onChange={(e) =>
                      setForm({
                        ...form,
                        args: e.target.value.split(/\s+/).filter(Boolean),
                      })
                    }
                  />
                  <div className="flex items-center gap-2">
                    <Button
                      variant="primary"
                      size="sm"
                      onClick={saveServer}
                      disabled={!form.name.trim() || !form.command.trim()}
                    >
                      {t("settings.mcp.form.save")}
                    </Button>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={testServer}
                      disabled={testing || !form.command.trim()}
                    >
                      <span className="inline-flex items-center gap-1.5">
                        <FlaskConical className="w-3.5 h-3.5" />
                        {testing
                          ? t("settings.mcp.form.testing")
                          : t("settings.mcp.form.test")}
                      </span>
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setForm(null)}
                    >
                      {t("settings.mcp.form.cancel")}
                    </Button>
                  </div>
                </div>
              )}
            </div>
          </SettingContainer>

          {catalog.length > 0 && (
            <SettingContainer
              title={t("settings.mcp.catalog.title")}
              description={t("settings.mcp.catalog.description")}
              descriptionMode="tooltip"
              grouped
              layout="stacked"
            >
              <div className="flex flex-col gap-1">
                {catalog.map((tool) => {
                  const toolKey = `${tool.server_id}/${tool.name}`;
                  const readOnly = tool.read_only_hint === true;
                  const approved = readOnly || autoApproved.includes(toolKey);
                  return (
                    <div
                      key={toolKey}
                      className="flex items-center justify-between gap-3 text-sm py-0.5"
                    >
                      <div className="min-w-0">
                        <span className="font-mono text-xs">{toolKey}</span>{" "}
                        <span
                          className={`text-[10px] uppercase tracking-wide rounded px-1 ${
                            readOnly
                              ? "bg-accent/20 text-accent"
                              : "bg-destructive/20 text-red-400"
                          }`}
                        >
                          {readOnly
                            ? t("settings.mcp.catalog.readOnly")
                            : t("settings.mcp.catalog.modifies")}
                        </span>
                      </div>
                      {!readOnly && (
                        <label className="flex items-center gap-1.5 text-xs text-muted-foreground whitespace-nowrap cursor-pointer">
                          <input
                            type="checkbox"
                            checked={approved}
                            onChange={(e) =>
                              toggleAutoApprove(toolKey, e.target.checked)
                            }
                          />
                          {t("settings.mcp.catalog.autoApprove")}
                        </label>
                      )}
                    </div>
                  );
                })}
              </div>
            </SettingContainer>
          )}
        </>
      )}
    </SettingsGroup>
  );
};
