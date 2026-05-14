import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Bot,
  CheckCircle2,
  Clock,
  KeyRound,
  Plug,
  Square,
  XCircle,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { ShortcutInput } from "../ShortcutInput";
import { SettingContainer } from "../../ui/SettingContainer";
import { SettingsGroup } from "../../ui/SettingsGroup";

type AgentSessionStatus = "idle" | "running";

interface AgentToolResult {
  toolName: string;
  output: string;
}

interface AgentSessionSnapshot {
  status: AgentSessionStatus;
  lastToolResult?: AgentToolResult | null;
}

interface AgentConnectionStatus {
  id: string;
  name: string;
  description: string;
  connected: boolean;
  missingEnv: string[];
  scopes: string[];
}

interface AgentEnvironment {
  openaiApiKeySaved: boolean;
  openaiRealtimeModel: string;
  googleOauthClientId: string;
  googleOauthClientSecretSaved: boolean;
  notionLeadsTableTarget: string;
  notionDealsTableTarget: string;
  notionCompaniesTableTarget: string;
  notionContactsTableTarget: string;
}

interface NotionTableValidation {
  dataSourceId: string;
}

const emptySnapshot: AgentSessionSnapshot = {
  status: "idle",
  lastToolResult: null,
};

const emptyEnvironment: AgentEnvironment = {
  openaiApiKeySaved: false,
  openaiRealtimeModel: "gpt-realtime",
  googleOauthClientId: "",
  googleOauthClientSecretSaved: false,
  notionLeadsTableTarget: "",
  notionDealsTableTarget: "",
  notionCompaniesTableTarget: "",
  notionContactsTableTarget: "",
};

async function invokeAgentCommand(command: string) {
  return invoke<AgentSessionSnapshot>(command);
}

export const AgentSettings: React.FC = () => {
  const { t } = useTranslation();
  const [session, setSession] = useState<AgentSessionSnapshot>(emptySnapshot);
  const [connections, setConnections] = useState<AgentConnectionStatus[]>([]);
  const [environment, setEnvironment] =
    useState<AgentEnvironment>(emptyEnvironment);
  const [draftEnvironment, setDraftEnvironment] =
    useState<AgentEnvironment>(emptyEnvironment);
  const [isBusy, setIsBusy] = useState(false);
  const [busyProviderId, setBusyProviderId] = useState<string | null>(null);
  const [isSavingEnvironment, setIsSavingEnvironment] = useState(false);
  const [validatingTableTarget, setValidatingTableTarget] = useState<
    string | null
  >(null);
  const [openaiApiKeyDraft, setOpenaiApiKeyDraft] = useState("");
  const [googleClientSecretDraft, setGoogleClientSecretDraft] = useState("");
  const isRunning = session.status === "running";

  useEffect(() => {
    invokeAgentCommand("get_agent_session")
      .then(setSession)
      .catch((error) => {
        console.warn("Failed to load agent session:", error);
      });
    invoke<AgentConnectionStatus[]>("get_agent_connections")
      .then(setConnections)
      .catch((error) => {
        console.warn("Failed to load agent connections:", error);
      });
    invoke<AgentEnvironment>("get_agent_environment")
      .then((nextEnvironment) => {
        setEnvironment(nextEnvironment);
        setDraftEnvironment(nextEnvironment);
      })
      .catch((error) => {
        console.warn("Failed to load agent environment:", error);
      });

    const unlisten = listen<AgentSessionSnapshot>(
      "agent-session-changed",
      (event) => {
        setSession(event.payload);
      },
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const runCommand = async (command: string) => {
    setIsBusy(true);
    try {
      const nextSession = await invokeAgentCommand(command);
      setSession(nextSession);
    } catch (error) {
      console.warn("Agent command failed:", error);
    } finally {
      setIsBusy(false);
    }
  };

  const connectProvider = async (providerId: string) => {
    setBusyProviderId(providerId);
    try {
      const nextConnections = await invoke<AgentConnectionStatus[]>(
        "connect_agent_provider",
        { providerId },
      );
      setConnections(nextConnections);
      toast.success(t("settings.agent.connections.connected"));
    } catch (error) {
      toast.error(t("settings.agent.connections.failed"), {
        description: String(error),
      });
    } finally {
      setBusyProviderId(null);
    }
  };

  const saveEnvironmentValue = async (key: string, value: string) => {
    setIsSavingEnvironment(true);
    try {
      const nextEnvironment = await invoke<AgentEnvironment>(
        "update_agent_environment_value",
        { key, value },
      );
      setEnvironment(nextEnvironment);
      setDraftEnvironment((current) => ({
        ...current,
        openaiApiKeySaved: nextEnvironment.openaiApiKeySaved,
        openaiRealtimeModel: nextEnvironment.openaiRealtimeModel,
        googleOauthClientId: nextEnvironment.googleOauthClientId,
        googleOauthClientSecretSaved:
          nextEnvironment.googleOauthClientSecretSaved,
        notionLeadsTableTarget: nextEnvironment.notionLeadsTableTarget,
        notionDealsTableTarget: nextEnvironment.notionDealsTableTarget,
        notionCompaniesTableTarget: nextEnvironment.notionCompaniesTableTarget,
        notionContactsTableTarget: nextEnvironment.notionContactsTableTarget,
      }));
      const nextConnections = await invoke<AgentConnectionStatus[]>(
        "get_agent_connections",
      );
      setConnections(nextConnections);
      if (key === "OPENAI_API_KEY") {
        setOpenaiApiKeyDraft("");
      }
      if (key === "GOOGLE_OAUTH_CLIENT_SECRET") {
        setGoogleClientSecretDraft("");
      }
    } catch (error) {
      toast.error(t("settings.agent.environment.failed"), {
        description: String(error),
      });
    } finally {
      setIsSavingEnvironment(false);
    }
  };

  const disconnectProvider = async (providerId: string) => {
    setBusyProviderId(providerId);
    try {
      const nextConnections = await invoke<AgentConnectionStatus[]>(
        "disconnect_agent_provider",
        { providerId },
      );
      setConnections(nextConnections);
    } catch (error) {
      toast.error(t("settings.agent.connections.failed"), {
        description: String(error),
      });
    } finally {
      setBusyProviderId(null);
    }
  };

  const validateTableTarget = async (id: string, target: string) => {
    setValidatingTableTarget(id);
    try {
      const validation = await invoke<NotionTableValidation>(
        "validate_agent_notion_table_target",
        { target },
      );
      toast.success(t("settings.agent.environment.tableValidated"), {
        description: validation.dataSourceId,
      });
    } catch (error) {
      toast.error(t("settings.agent.environment.tableValidationFailed"), {
        description: String(error),
      });
    } finally {
      setValidatingTableTarget(null);
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup
        title={t("settings.agent.title")}
        description={t("settings.agent.description")}
      >
        <SettingContainer
          title={t("settings.agent.session.title")}
          description={t("settings.agent.session.description")}
          grouped={true}
          layout="stacked"
          descriptionMode="inline"
        >
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              size="md"
              disabled={isBusy || isRunning}
              onClick={() => runCommand("start_agent_session")}
            >
              <Bot className="inline-block w-4 h-4 me-1 align-text-bottom" />
              {t("settings.agent.session.start")}
            </Button>
            <Button
              variant="secondary"
              size="md"
              disabled={isBusy || !isRunning}
              onClick={() => runCommand("stop_agent_session")}
            >
              <Square className="inline-block w-4 h-4 me-1 align-text-bottom" />
              {t("settings.agent.session.stop")}
            </Button>
            <span className="text-sm text-mid-gray">
              {isRunning
                ? t("settings.agent.session.running")
                : t("settings.agent.session.idle")}
            </span>
          </div>
        </SettingContainer>
        <ShortcutInput
          shortcutId="agent"
          grouped={true}
          descriptionMode="inline"
        />

        <SettingContainer
          title={t("settings.agent.environment.title")}
          description={t("settings.agent.environment.description")}
          grouped={true}
          layout="stacked"
          descriptionMode="inline"
        >
          <div className="grid gap-3">
            <label className="grid gap-1">
              <span className="flex items-center gap-2 text-sm font-medium">
                <KeyRound className="h-4 w-4" />
                {t("settings.agent.environment.openaiApiKey")}
              </span>
              <Input
                type="password"
                value={openaiApiKeyDraft}
                disabled={isSavingEnvironment}
                placeholder={
                  environment.openaiApiKeySaved
                    ? t("settings.agent.environment.savedPlaceholder")
                    : t("settings.agent.environment.emptyPlaceholder")
                }
                onChange={(event) => setOpenaiApiKeyDraft(event.target.value)}
                onBlur={(event) => {
                  if (event.target.value) {
                    saveEnvironmentValue("OPENAI_API_KEY", event.target.value);
                  }
                }}
              />
            </label>
            <label className="grid gap-1">
              <span className="text-sm font-medium">
                {t("settings.agent.environment.realtimeModel")}
              </span>
              <Input
                value={draftEnvironment.openaiRealtimeModel}
                disabled={isSavingEnvironment}
                onChange={(event) =>
                  setDraftEnvironment((current) => ({
                    ...current,
                    openaiRealtimeModel: event.target.value,
                  }))
                }
                onBlur={(event) =>
                  saveEnvironmentValue(
                    "OPENAI_REALTIME_MODEL",
                    event.target.value,
                  )
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-sm font-medium">
                {t("settings.agent.environment.googleClientId")}
              </span>
              <Input
                value={draftEnvironment.googleOauthClientId}
                disabled={isSavingEnvironment}
                placeholder={t("settings.agent.environment.googleClientId")}
                onChange={(event) =>
                  setDraftEnvironment((current) => ({
                    ...current,
                    googleOauthClientId: event.target.value,
                  }))
                }
                onBlur={(event) =>
                  saveEnvironmentValue(
                    "GOOGLE_OAUTH_CLIENT_ID",
                    event.target.value,
                  )
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-sm font-medium">
                {t("settings.agent.environment.googleClientSecret")}
              </span>
              <Input
                type="password"
                value={googleClientSecretDraft}
                disabled={isSavingEnvironment}
                placeholder={
                  environment.googleOauthClientSecretSaved
                    ? t("settings.agent.environment.savedPlaceholder")
                    : t("settings.agent.environment.emptyPlaceholder")
                }
                onChange={(event) =>
                  setGoogleClientSecretDraft(event.target.value)
                }
                onBlur={(event) => {
                  if (event.target.value) {
                    saveEnvironmentValue(
                      "GOOGLE_OAUTH_CLIENT_SECRET",
                      event.target.value,
                    );
                  }
                }}
              />
            </label>
            <label className="grid gap-1">
              <span className="text-sm font-medium">
                {t("settings.agent.environment.notionLeadsTableTarget")}
              </span>
              <div className="flex gap-2">
                <Input
                  value={draftEnvironment.notionLeadsTableTarget}
                  disabled={isSavingEnvironment}
                  placeholder={t(
                    "settings.agent.environment.notionTableTargetPlaceholder",
                  )}
                  onChange={(event) =>
                    setDraftEnvironment((current) => ({
                      ...current,
                      notionLeadsTableTarget: event.target.value,
                    }))
                  }
                  onBlur={(event) =>
                    saveEnvironmentValue(
                      "NOTION_LEADS_TABLE_TARGET",
                      event.target.value,
                    )
                  }
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={
                    !draftEnvironment.notionLeadsTableTarget ||
                    validatingTableTarget === "leads"
                  }
                  onClick={() =>
                    validateTableTarget(
                      "leads",
                      draftEnvironment.notionLeadsTableTarget,
                    )
                  }
                >
                  {t("settings.agent.environment.validateTable")}
                </Button>
              </div>
            </label>
            <label className="grid gap-1">
              <span className="text-sm font-medium">
                {t("settings.agent.environment.notionDealsTableTarget")}
              </span>
              <div className="flex gap-2">
                <Input
                  value={draftEnvironment.notionDealsTableTarget}
                  disabled={isSavingEnvironment}
                  placeholder={t(
                    "settings.agent.environment.notionTableTargetPlaceholder",
                  )}
                  onChange={(event) =>
                    setDraftEnvironment((current) => ({
                      ...current,
                      notionDealsTableTarget: event.target.value,
                    }))
                  }
                  onBlur={(event) =>
                    saveEnvironmentValue(
                      "NOTION_DEALS_TABLE_TARGET",
                      event.target.value,
                    )
                  }
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={
                    !draftEnvironment.notionDealsTableTarget ||
                    validatingTableTarget === "deals"
                  }
                  onClick={() =>
                    validateTableTarget(
                      "deals",
                      draftEnvironment.notionDealsTableTarget,
                    )
                  }
                >
                  {t("settings.agent.environment.validateTable")}
                </Button>
              </div>
            </label>
            <label className="grid gap-1">
              <span className="text-sm font-medium">
                {t("settings.agent.environment.notionCompaniesTableTarget")}
              </span>
              <div className="flex gap-2">
                <Input
                  value={draftEnvironment.notionCompaniesTableTarget}
                  disabled={isSavingEnvironment}
                  placeholder={t(
                    "settings.agent.environment.notionTableTargetPlaceholder",
                  )}
                  onChange={(event) =>
                    setDraftEnvironment((current) => ({
                      ...current,
                      notionCompaniesTableTarget: event.target.value,
                    }))
                  }
                  onBlur={(event) =>
                    saveEnvironmentValue(
                      "NOTION_COMPANIES_TABLE_TARGET",
                      event.target.value,
                    )
                  }
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={
                    !draftEnvironment.notionCompaniesTableTarget ||
                    validatingTableTarget === "companies"
                  }
                  onClick={() =>
                    validateTableTarget(
                      "companies",
                      draftEnvironment.notionCompaniesTableTarget,
                    )
                  }
                >
                  {t("settings.agent.environment.validateTable")}
                </Button>
              </div>
            </label>
            <label className="grid gap-1">
              <span className="text-sm font-medium">
                {t("settings.agent.environment.notionContactsTableTarget")}
              </span>
              <div className="flex gap-2">
                <Input
                  value={draftEnvironment.notionContactsTableTarget}
                  disabled={isSavingEnvironment}
                  placeholder={t(
                    "settings.agent.environment.notionTableTargetPlaceholder",
                  )}
                  onChange={(event) =>
                    setDraftEnvironment((current) => ({
                      ...current,
                      notionContactsTableTarget: event.target.value,
                    }))
                  }
                  onBlur={(event) =>
                    saveEnvironmentValue(
                      "NOTION_CONTACTS_TABLE_TARGET",
                      event.target.value,
                    )
                  }
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={
                    !draftEnvironment.notionContactsTableTarget ||
                    validatingTableTarget === "contacts"
                  }
                  onClick={() =>
                    validateTableTarget(
                      "contacts",
                      draftEnvironment.notionContactsTableTarget,
                    )
                  }
                >
                  {t("settings.agent.environment.validateTable")}
                </Button>
              </div>
            </label>
          </div>
        </SettingContainer>

        <SettingContainer
          title={t("settings.agent.connections.title")}
          description={t("settings.agent.connections.description")}
          grouped={true}
          layout="stacked"
          descriptionMode="inline"
        >
          <div className="grid gap-3">
            {connections.map((connection) => {
              const isProviderBusy = busyProviderId === connection.id;
              const isMissingEnv = connection.missingEnv.length > 0;
              return (
                <div
                  key={connection.id}
                  className="rounded-lg border border-mid-gray/20 bg-mid-gray/5 p-3"
                >
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2 text-sm font-semibold">
                        {connection.connected ? (
                          <CheckCircle2 className="h-4 w-4 text-green-600" />
                        ) : (
                          <XCircle className="h-4 w-4 text-mid-gray" />
                        )}
                        <span>{connection.name}</span>
                      </div>
                      <div className="mt-1 text-sm text-mid-gray">
                        {connection.description}
                      </div>
                      {isMissingEnv && (
                        <div className="mt-2 text-xs text-red-600">
                          {t("settings.agent.connections.missingEnv", {
                            env: connection.missingEnv.join(", "),
                          })}
                        </div>
                      )}
                    </div>
                    <Button
                      variant={connection.connected ? "secondary" : "primary"}
                      size="sm"
                      disabled={isProviderBusy || isMissingEnv}
                      onClick={() =>
                        connection.connected
                          ? disconnectProvider(connection.id)
                          : connectProvider(connection.id)
                      }
                    >
                      <Plug className="inline-block w-4 h-4 me-1 align-text-bottom" />
                      {connection.connected
                        ? t("settings.agent.connections.disconnect")
                        : t("settings.agent.connections.connect")}
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        </SettingContainer>

        <SettingContainer
          title={t("settings.agent.tools.title")}
          description={t("settings.agent.tools.description")}
          grouped={true}
          layout="stacked"
          descriptionMode="inline"
        >
          <div className="space-y-3">
            <Button
              variant="secondary"
              size="md"
              disabled={isBusy}
              onClick={() => runCommand("run_agent_test_tool")}
            >
              <Clock className="inline-block w-4 h-4 me-1 align-text-bottom" />
              {t("settings.agent.tools.runTimeTool")}
            </Button>
            <div className="rounded-lg border border-mid-gray/20 bg-mid-gray/5 p-3">
              <div className="text-xs uppercase tracking-wide text-mid-gray">
                {t("settings.agent.tools.lastResult")}
              </div>
              <div className="mt-1 text-sm font-mono break-all">
                {session.lastToolResult
                  ? `${session.lastToolResult.toolName}: ${session.lastToolResult.output}`
                  : t("settings.agent.tools.noResult")}
              </div>
            </div>
          </div>
        </SettingContainer>
      </SettingsGroup>
    </div>
  );
};
