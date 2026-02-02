import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";

interface SonioxSettingsProps {
	descriptionMode?: "inline" | "tooltip";
	grouped?: boolean;
}

export const SonioxSettings: React.FC<SonioxSettingsProps> = React.memo(
	({ descriptionMode = "tooltip", grouped = false }) => {
		const { t } = useTranslation();
		const { getSetting, refreshSettings, isUpdating } = useSettings();

		const apiKey = (getSetting("soniox_api_key") || "") as string;
		const selectedModel = (getSetting("soniox_model") ||
			"stt-async-v4") as string;
		const timeoutSeconds = (getSetting("soniox_timeout_seconds") ||
			120) as number;

		const [localApiKey, setLocalApiKey] = useState(apiKey);
		const [localTimeout, setLocalTimeout] = useState(timeoutSeconds);

		// Sync local state when settings change
		React.useEffect(() => {
			setLocalApiKey(apiKey);
		}, [apiKey]);

		React.useEffect(() => {
			setLocalTimeout(timeoutSeconds);
		}, [timeoutSeconds]);

		const handleApiKeyBlur = async () => {
			if (localApiKey === apiKey) return;

			try {
				const result = await commands.setSonioxApiKey(localApiKey);
				if (result.status === "ok") {
					await refreshSettings();
				}
			} catch (error) {
				console.error("Failed to save Soniox API key:", error);
			}
		};

		const handleModelChange = async (model: string) => {
			try {
				const result = await commands.setSonioxModel(model);
				if (result.status === "ok") {
					await refreshSettings();
				}
			} catch (error) {
				console.error("Failed to change Soniox model:", error);
			}
		};

		const handleTimeoutBlur = async () => {
			if (localTimeout === timeoutSeconds) return;

			// Clamp to valid range
			const clampedTimeout = Math.min(300, Math.max(30, localTimeout));
			setLocalTimeout(clampedTimeout);

			try {
				const result = await commands.setSonioxTimeout(clampedTimeout);
				if (result.status === "ok") {
					await refreshSettings();
				}
			} catch (error) {
				console.error("Failed to save Soniox timeout:", error);
				// Revert on error
				setLocalTimeout(timeoutSeconds);
			}
		};

		const modelOptions = [
			{
				value: "stt-async-v4",
				label: t("soniox.model.options.stt-async-v4"),
			},
		];

		const containerClasses = grouped
			? "space-y-4 p-4"
			: "space-y-4 p-4 rounded-lg border border-mid-gray/20";

		return (
			<div className={containerClasses}>
				<h3 className="text-sm font-medium">{t("soniox.title")}</h3>

				{/* API Key */}
				<SettingContainer
					title={t("soniox.apiKey.label")}
					description={t("soniox.apiKey.description")}
					descriptionMode={descriptionMode}
					grouped={false}
					layout="stacked"
				>
					<input
						type="password"
						value={localApiKey}
						onChange={(e) => setLocalApiKey(e.target.value)}
						onBlur={handleApiKeyBlur}
						placeholder={t("soniox.apiKey.placeholder")}
						disabled={isUpdating("soniox_api_key")}
						className="w-full px-3 py-2 rounded-lg border border-mid-gray/30 bg-transparent text-sm focus:outline-none focus:border-logo-primary transition-colors"
					/>
				</SettingContainer>

				{/* Model Selection */}
				<SettingContainer
					title={t("soniox.model.label")}
					description={t("soniox.model.description")}
					descriptionMode={descriptionMode}
					grouped={false}
				>
					<Dropdown
						options={modelOptions}
						selectedValue={selectedModel}
						onSelect={handleModelChange}
						disabled={isUpdating("soniox_model")}
					/>
				</SettingContainer>

				{/* Timeout */}
				<SettingContainer
					title={t("soniox.timeout.label")}
					description={t("soniox.timeout.description")}
					descriptionMode={descriptionMode}
					grouped={false}
				>
					<div className="flex items-center gap-2">
						<input
							type="number"
							min={30}
							max={300}
							value={localTimeout}
							onChange={(e) => setLocalTimeout(parseInt(e.target.value) || 30)}
							onBlur={handleTimeoutBlur}
							disabled={isUpdating("soniox_timeout_seconds")}
							className="w-24 px-3 py-2 rounded-lg border border-mid-gray/30 bg-transparent text-sm focus:outline-none focus:border-logo-primary transition-colors"
						/>
						<span className="text-sm text-mid-gray">
							{t("soniox.timeout.unit")}
						</span>
					</div>
				</SettingContainer>
			</div>
		);
	},
);
