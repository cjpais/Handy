import React, { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import {
  CheckCircle2,
  RefreshCcw,
  RotateCcw,
  TriangleAlert,
  X,
} from "lucide-react";
import { commands, type PostProcessTestFailure } from "@/bindings";

import { Alert } from "../../ui/Alert";
import {
  Dropdown,
  SettingContainer,
  SettingsGroup,
  Textarea,
} from "@/components/ui";
import { Button } from "../../ui/Button";
import { ResetButton } from "../../ui/ResetButton";
import { Input } from "../../ui/Input";

import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { ShortcutInput } from "../ShortcutInput";
import { useSettings } from "../../../hooks/useSettings";

type TestResultState =
  | { kind: "success"; model: string }
  | { kind: "error"; model: string; failure: PostProcessTestFailure };

// Maps the wire value from PostProcessTestErrorKind to its translation key.
// Unknown kinds fall back to the generic provider error, so a newer backend
// still renders something readable instead of a raw key.
const TEST_ERROR_KEYS: Record<string, string> = {
  authentication: "authentication",
  "model-not-found": "modelNotFound",
  "rate-limited": "rateLimited",
  provider: "provider",
  timeout: "timeout",
  "unexpected-response": "unexpectedResponse",
  "empty-response": "emptyResponse",
  "no-provider": "noProvider",
  "no-model": "noModel",
  "missing-api-key": "missingApiKey",
  "unsupported-platform": "unsupportedPlatform",
};

const PostProcessingSettingsApiComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();
  const [testingModel, setTestingModel] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<TestResultState | null>(null);

  useEffect(() => {
    setTestingModel(null);
    setTestResult(null);
  }, [state.selectedProviderId]);

  const handleTestModel = async (model?: string) => {
    const targetModel = model || state.model;
    if (testingModel || !targetModel) return;

    setTestingModel(targetModel);
    setTestResult(null);
    try {
      const result = await commands.testPostProcessModel(targetModel);
      if (result.status === "ok") {
        setTestResult({ kind: "success", model: targetModel });
      } else {
        setTestResult({
          kind: "error",
          model: targetModel,
          failure: result.error,
        });
      }
    } catch (error) {
      // A thrown error means the command never produced a classified failure,
      // so surface it verbatim rather than pretending it is a provider problem.
      setTestResult({
        kind: "error",
        model: targetModel,
        failure: { kind: "provider", detail: String(error) },
      });
    } finally {
      setTestingModel(null);
    }
  };

  return (
    <>
      <SettingContainer
        title={t("settings.postProcessing.api.provider.title")}
        description={t("settings.postProcessing.api.provider.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ProviderSelect
            options={state.providerOptions}
            value={state.selectedProviderId}
            onChange={state.handleProviderSelect}
          />
        </div>
      </SettingContainer>

      {state.isAppleProvider ? (
        state.appleIntelligenceUnavailable ? (
          <Alert variant="error" contained>
            {t("settings.postProcessing.api.appleIntelligence.unavailable")}
          </Alert>
        ) : null
      ) : (
        <>
          {state.selectedProvider?.id === "custom" && (
            <SettingContainer
              title={t("settings.postProcessing.api.baseUrl.title")}
              description={t("settings.postProcessing.api.baseUrl.description")}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <BaseUrlField
                  value={state.baseUrl}
                  onBlur={state.handleBaseUrlChange}
                  placeholder={t(
                    "settings.postProcessing.api.baseUrl.placeholder",
                  )}
                  disabled={state.isBaseUrlUpdating}
                  className="min-w-[380px]"
                />
              </div>
            </SettingContainer>
          )}

          <SettingContainer
            title={t("settings.postProcessing.api.apiKey.title")}
            description={t("settings.postProcessing.api.apiKey.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <div className="flex items-center gap-2">
              <ApiKeyField
                value={state.apiKey}
                onBlur={state.handleApiKeyChange}
                placeholder={t(
                  "settings.postProcessing.api.apiKey.placeholder",
                )}
                disabled={state.isApiKeyUpdating}
                className="min-w-[320px]"
              />
            </div>
          </SettingContainer>
        </>
      )}

      {!state.isAppleProvider && (
        <SettingContainer
          title={t("settings.postProcessing.api.model.title")}
          description={
            state.isCustomProvider
              ? t("settings.postProcessing.api.model.descriptionCustom")
              : t("settings.postProcessing.api.model.descriptionDefault")
          }
          descriptionMode="tooltip"
          layout="stacked"
          grouped={true}
        >
          <div className="space-y-3">
            <div className="flex min-w-0 items-center gap-2">
              <ModelSelect
                value={state.model}
                options={state.modelOptions}
                disabled={state.isModelUpdating}
                isLoading={state.isFetchingModels}
                placeholder={
                  state.modelOptions.length > 0
                    ? t(
                        "settings.postProcessing.api.model.placeholderWithOptions",
                      )
                    : t(
                        "settings.postProcessing.api.model.placeholderNoOptions",
                      )
                }
                onSelect={state.handleModelSelect}
                onCreate={state.handleModelCreate}
                onBlur={() => {}}
                onTest={handleTestModel}
                testingModel={testingModel}
                testLabel={t("settings.postProcessing.api.model.test")}
                testingLabel={t("settings.postProcessing.api.model.testing")}
                className="min-w-0 flex-1"
              />
              <ResetButton
                onClick={state.handleRefreshModels}
                disabled={state.isFetchingModels}
                ariaLabel={t("settings.postProcessing.api.model.refreshModels")}
                className="flex h-10 w-10 shrink-0 items-center justify-center"
              >
                <RefreshCcw
                  className={`h-4 w-4 ${state.isFetchingModels ? "animate-spin" : ""}`}
                />
              </ResetButton>
            </div>

            {testResult && (
              <div
                className={`flex items-start gap-3 rounded-lg border px-3 py-2.5 ${
                  testResult.kind === "error"
                    ? "border-error/40 bg-error/10"
                    : "border-success/40 bg-success/10"
                }`}
              >
                {testResult.kind === "error" ? (
                  <TriangleAlert className="mt-0.5 h-5 w-5 shrink-0 text-error" />
                ) : (
                  <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-success" />
                )}
                <div className="min-w-0 flex-1 space-y-1">
                  {testResult.kind === "error" ? (
                    <>
                      <p className="text-sm font-medium leading-5 text-error">
                        {t(
                          `settings.postProcessing.api.model.errors.${
                            TEST_ERROR_KEYS[testResult.failure.kind] ??
                            "provider"
                          }`,
                        )}
                      </p>
                      <p className="break-words text-xs leading-4 text-error/80">
                        {testResult.failure.detail}
                      </p>
                    </>
                  ) : (
                    <p className="text-sm leading-5 text-success">
                      {t("settings.postProcessing.api.model.testSuccess")}
                    </p>
                  )}
                  <div className="flex items-center gap-1 pt-1">
                    {testResult.kind === "error" && (
                      <button
                        type="button"
                        onClick={() => handleTestModel(testResult.model)}
                        disabled={Boolean(testingModel)}
                        className="inline-flex cursor-pointer items-center gap-1.5 whitespace-nowrap rounded px-2 py-1 text-xs font-medium text-error transition-colors hover:bg-error/15 focus:outline-none focus:ring-1 focus:ring-error disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        <RotateCcw
                          className={`h-3.5 w-3.5 ${
                            testingModel === testResult.model
                              ? "animate-spin"
                              : ""
                          }`}
                        />
                        {t("settings.postProcessing.api.model.retry")}
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => setTestResult(null)}
                      aria-label={t(
                        "settings.postProcessing.api.model.dismiss",
                      )}
                      className={`cursor-pointer rounded p-1 transition-colors focus:outline-none focus:ring-1 ${
                        testResult.kind === "error"
                          ? "text-error hover:bg-error/15 focus:ring-error"
                          : "text-success hover:bg-success/15 focus:ring-success"
                      }`}
                    >
                      <X className="h-4 w-4" />
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
        </SettingContainer>
      )}
    </>
  );
};

const PostProcessingFallbackComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();
  const [testingFallbackModel, setTestingFallbackModel] = useState<string | null>(
    null,
  );
  const [fallbackTestResult, setFallbackTestResult] =
    useState<TestResultState | null>(null);

  useEffect(() => {
    setTestingFallbackModel(null);
    setFallbackTestResult(null);
  }, [state.selectedProviderId]);

  const handleTestFallbackModel = async (model?: string) => {
    const targetModel = model || state.fallbackModel;
    if (testingFallbackModel || !targetModel) return;

    setTestingFallbackModel(targetModel);
    setFallbackTestResult(null);
    try {
      const result = await commands.testPostProcessModel(targetModel);
      if (result.status === "ok") {
        setFallbackTestResult({ kind: "success", model: targetModel });
      } else {
        setFallbackTestResult({
          kind: "error",
          model: targetModel,
          failure: result.error,
        });
      }
    } catch (error) {
      setFallbackTestResult({
        kind: "error",
        model: targetModel,
        failure: { kind: "provider", detail: String(error) },
      });
    } finally {
      setTestingFallbackModel(null);
    }
  };

  return (
    <>
      <ToggleSwitch
        checked={state.fallbackEnabled}
        onChange={state.handleFallbackEnabledChange}
        isUpdating={state.isFallbackEnabledUpdating}
        label={t("settings.postProcessing.fallback.enabled.title")}
        description={t("settings.postProcessing.fallback.enabled.description")}
        descriptionMode="tooltip"
        grouped={true}
      />

      {state.fallbackEnabled && !state.isAppleProvider && (
        <>
          <SettingContainer
            title={t("settings.postProcessing.fallback.model.title")}
            description={
              state.isCustomProvider
                ? t(
                    "settings.postProcessing.fallback.model.descriptionCustom",
                  )
                : t(
                    "settings.postProcessing.fallback.model.descriptionDefault",
                  )
            }
            descriptionMode="tooltip"
            layout="stacked"
            grouped={true}
          >
            <div className="space-y-3">
              <div className="flex items-center gap-2">
                <ModelSelect
                  value={state.fallbackModel}
                  options={state.fallbackModelOptions}
                  disabled={state.isFallbackModelUpdating}
                  isLoading={state.isFetchingModels}
                  placeholder={
                    state.fallbackModelOptions.length > 0
                      ? t(
                          "settings.postProcessing.fallback.model.placeholderWithOptions",
                        )
                      : t(
                          "settings.postProcessing.fallback.model.placeholderNoOptions",
                        )
                  }
                  onSelect={state.handleFallbackModelSelect}
                  onCreate={state.handleFallbackModelCreate}
                  onBlur={() => {}}
                  onTest={handleTestFallbackModel}
                  testingModel={testingFallbackModel}
                  testLabel={t("settings.postProcessing.api.model.test")}
                  testingLabel={t("settings.postProcessing.api.model.testing")}
                  className="flex-1 min-w-[380px]"
                />
              </div>

              {fallbackTestResult && (
                <div
                  className={`flex items-start gap-3 rounded-lg border px-3 py-2.5 ${
                    fallbackTestResult.kind === "error"
                      ? "border-error/40 bg-error/10"
                      : "border-success/40 bg-success/10"
                  }`}
                >
                  {fallbackTestResult.kind === "error" ? (
                    <TriangleAlert className="mt-0.5 h-5 w-5 shrink-0 text-error" />
                  ) : (
                    <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-success" />
                  )}
                  <div className="min-w-0 flex-1 space-y-1">
                    {fallbackTestResult.kind === "error" ? (
                      <>
                        <p className="text-sm font-medium leading-5 text-error">
                          {t(
                            `settings.postProcessing.api.model.errors.${
                              TEST_ERROR_KEYS[fallbackTestResult.failure.kind] ??
                              "provider"
                            }`,
                          )}
                        </p>
                        <p className="break-words text-xs leading-4 text-error/80">
                          {fallbackTestResult.failure.detail}
                        </p>
                      </>
                    ) : (
                      <p className="text-sm leading-5 text-success">
                        {t("settings.postProcessing.api.model.testSuccess")}
                      </p>
                    )}
                    <div className="flex items-center gap-1 pt-1">
                      {fallbackTestResult.kind === "error" && (
                        <button
                          type="button"
                          onClick={() =>
                            handleTestFallbackModel(fallbackTestResult.model)
                          }
                          disabled={Boolean(testingFallbackModel)}
                          className="inline-flex cursor-pointer items-center gap-1.5 whitespace-nowrap rounded px-2 py-1 text-xs font-medium text-error transition-colors hover:bg-error/15 focus:outline-none focus:ring-1 focus:ring-error disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          <RotateCcw
                            className={`h-3.5 w-3.5 ${
                              testingFallbackModel === fallbackTestResult.model
                                ? "animate-spin"
                                : ""
                            }`}
                          />
                          {t("settings.postProcessing.api.model.retry")}
                        </button>
                      )}
                      <button
                        type="button"
                        onClick={() => setFallbackTestResult(null)}
                        aria-label={t(
                          "settings.postProcessing.api.model.dismiss",
                        )}
                        className={`cursor-pointer rounded p-1 transition-colors focus:outline-none focus:ring-1 ${
                          fallbackTestResult.kind === "error"
                            ? "text-error hover:bg-error/15 focus:ring-error"
                            : "text-success hover:bg-success/15 focus:ring-success"
                        }`}
                      >
                        <X className="h-4 w-4" />
                      </button>
                    </div>
                  </div>
                </div>
              )}
            </div>
          </SettingContainer>

          <ToggleSwitch
            checked={state.fallbackUseSeparateKey}
            onChange={state.handleFallbackUseSeparateKeyChange}
            isUpdating={state.isFallbackSeparateKeyUpdating}
            label={t("settings.postProcessing.fallback.separateKey.title")}
            description={t(
              "settings.postProcessing.fallback.separateKey.description",
            )}
            descriptionMode="tooltip"
            grouped={true}
          />

          {state.fallbackUseSeparateKey && (
            <SettingContainer
              title={t("settings.postProcessing.fallback.apiKey.title")}
              description={t(
                "settings.postProcessing.fallback.apiKey.description",
              )}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <ApiKeyField
                  value={state.fallbackApiKey}
                  onBlur={state.handleFallbackApiKeyChange}
                  placeholder={t(
                    "settings.postProcessing.fallback.apiKey.placeholder",
                  )}
                  disabled={state.isFallbackApiKeyUpdating}
                  className="min-w-[320px]"
                />
              </div>
            </SettingContainer>
          )}
        </>
      )}
    </>
  );
};

const PostProcessingSettingsPromptsComponent: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");

  const prompts = getSetting("post_process_prompts") || [];
  const selectedPromptId = getSetting("post_process_selected_prompt_id") || "";
  const selectedPrompt =
    prompts.find((prompt) => prompt.id === selectedPromptId) || null;

  useEffect(() => {
    if (isCreating) return;

    if (selectedPrompt) {
      setDraftName(selectedPrompt.name);
      setDraftText(selectedPrompt.prompt);
    } else {
      setDraftName("");
      setDraftText("");
    }
  }, [
    isCreating,
    selectedPromptId,
    selectedPrompt?.name,
    selectedPrompt?.prompt,
  ]);

  const handlePromptSelect = (promptId: string | null) => {
    if (!promptId) return;
    updateSetting("post_process_selected_prompt_id", promptId);
    setIsCreating(false);
  };

  const handleCreatePrompt = async () => {
    if (!draftName.trim() || !draftText.trim()) return;

    try {
      const result = await commands.addPostProcessPrompt(
        draftName.trim(),
        draftText.trim(),
      );
      if (result.status === "ok") {
        await refreshSettings();
        updateSetting("post_process_selected_prompt_id", result.data.id);
        setIsCreating(false);
      }
    } catch (error) {
      console.error("Failed to create prompt:", error);
    }
  };

  const handleUpdatePrompt = async () => {
    if (!selectedPromptId || !draftName.trim() || !draftText.trim()) return;

    try {
      await commands.updatePostProcessPrompt(
        selectedPromptId,
        draftName.trim(),
        draftText.trim(),
      );
      await refreshSettings();
    } catch (error) {
      console.error("Failed to update prompt:", error);
    }
  };

  const handleDeletePrompt = async (promptId: string) => {
    if (!promptId) return;

    try {
      await commands.deletePostProcessPrompt(promptId);
      await refreshSettings();
      setIsCreating(false);
    } catch (error) {
      console.error("Failed to delete prompt:", error);
    }
  };

  const handleCancelCreate = () => {
    setIsCreating(false);
    if (selectedPrompt) {
      setDraftName(selectedPrompt.name);
      setDraftText(selectedPrompt.prompt);
    } else {
      setDraftName("");
      setDraftText("");
    }
  };

  const handleStartCreate = () => {
    setIsCreating(true);
    setDraftName("");
    setDraftText("");
  };

  const hasPrompts = prompts.length > 0;
  const isDirty =
    !!selectedPrompt &&
    (draftName.trim() !== selectedPrompt.name ||
      draftText.trim() !== selectedPrompt.prompt.trim());

  return (
    <SettingContainer
      title={t("settings.postProcessing.prompts.selectedPrompt.title")}
      description={t(
        "settings.postProcessing.prompts.selectedPrompt.description",
      )}
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <div className="space-y-3">
        <div className="flex gap-2 min-w-0">
          <Dropdown
            selectedValue={selectedPromptId || null}
            options={prompts.map((p) => ({
              value: p.id,
              label: p.name,
            }))}
            onSelect={(value) => handlePromptSelect(value)}
            placeholder={
              prompts.length === 0
                ? t("settings.postProcessing.prompts.noPrompts")
                : t("settings.postProcessing.prompts.selectPrompt")
            }
            disabled={
              isUpdating("post_process_selected_prompt_id") || isCreating
            }
            className="flex-1 min-w-0"
          />
          <Button
            onClick={handleStartCreate}
            variant="primary"
            size="md"
            disabled={isCreating}
            className="shrink-0"
          >
            {t("settings.postProcessing.prompts.createNew")}
          </Button>
        </div>

        {!isCreating && hasPrompts && selectedPrompt && (
          <div className="space-y-3">
            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptLabel")}
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptLabelPlaceholder",
                )}
                variant="compact"
              />
            </div>

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptInstructions")}
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                )}
              />
              <p className="text-xs text-mid-gray/70">
                <Trans
                  i18nKey="settings.postProcessing.prompts.promptTip"
                  components={{ code: <code /> }}
                />
              </p>
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleUpdatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim() || !isDirty}
              >
                {t("settings.postProcessing.prompts.updatePrompt")}
              </Button>
              <Button
                onClick={() => handleDeletePrompt(selectedPromptId)}
                variant="secondary"
                size="md"
                disabled={!selectedPromptId || prompts.length <= 1}
              >
                {t("settings.postProcessing.prompts.deletePrompt")}
              </Button>
            </div>
          </div>
        )}

        {!isCreating && !selectedPrompt && (
          <div className="p-3 bg-mid-gray/5 rounded-md border border-mid-gray/20">
            <p className="text-sm text-mid-gray">
              {hasPrompts
                ? t("settings.postProcessing.prompts.selectToEdit")
                : t("settings.postProcessing.prompts.createFirst")}
            </p>
          </div>
        )}

        {isCreating && (
          <div className="space-y-3">
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-text">
                {t("settings.postProcessing.prompts.promptLabel")}
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptLabelPlaceholder",
                )}
                variant="compact"
              />
            </div>

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptInstructions")}
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                )}
              />
              <p className="text-xs text-mid-gray/70">
                <Trans
                  i18nKey="settings.postProcessing.prompts.promptTip"
                  components={{ code: <code /> }}
                />
              </p>
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleCreatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim()}
              >
                {t("settings.postProcessing.prompts.createPrompt")}
              </Button>
              <Button
                onClick={handleCancelCreate}
                variant="secondary"
                size="md"
              >
                {t("settings.postProcessing.prompts.cancel")}
              </Button>
            </div>
          </div>
        )}
      </div>
    </SettingContainer>
  );
};

export const PostProcessingFallback: React.FC = () => {
  const { t } = useTranslation();

  return (
    <SettingsGroup title={t("settings.postProcessing.fallback.title")}>
      <PostProcessingFallbackComponent />
    </SettingsGroup>
  );
};

export const PostProcessingSettingsApi = React.memo(
  PostProcessingSettingsApiComponent,
);
PostProcessingSettingsApi.displayName = "PostProcessingSettingsApi";

export const PostProcessingSettingsPrompts = React.memo(
  PostProcessingSettingsPromptsComponent,
);
PostProcessingSettingsPrompts.displayName = "PostProcessingSettingsPrompts";

export const PostProcessingSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.postProcessing.hotkey.title")}>
        <ShortcutInput
          shortcutId="transcribe_with_post_process"
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.api.title")}>
        <PostProcessingSettingsApi />
      </SettingsGroup>

      <PostProcessingFallback />

      <SettingsGroup title={t("settings.postProcessing.prompts.title")}>
        <PostProcessingSettingsPrompts />
      </SettingsGroup>
    </div>
  );
};
