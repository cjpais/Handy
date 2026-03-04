import { listen as subscribeEvent } from "@tauri-apps/api/event";
import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettingsStore } from "../../../stores/settingsStore";
import { Dropdown } from "../../ui/Dropdown";
import { ResetButton } from "../../ui/ResetButton";
import { SettingContainer } from "../../ui/SettingContainer";

type MidiSettingsPayload = {
	enabled: boolean;
	deviceName: string | null;
	trigger: number[] | null;
};

export const MidiSettings: React.FC = React.memo(() => {
	const { t } = useTranslation();
	const settings = useSettingsStore((state) => state.settings);
	const setSettings = useSettingsStore((state) => state.setSettings);
	const refreshSettings = useSettingsStore((state) => state.refreshSettings);

	const [ports, setPorts] = useState<string[]>([]);
	const [isBinding, setIsBinding] = useState(false);
	const [isSaving, setIsSaving] = useState(false);
	const isBindingRef = useRef(false);
	const [selectedDeviceLocal, setSelectedDeviceLocal] = useState<string | null>(
		settings?.midi_device_name ?? null,
	);
	const selectedDeviceRef = useRef<string | null>(
		settings?.midi_device_name ?? null,
	);

	const selectedDevice = settings?.midi_device_name ?? null;
	const midiTrigger = settings?.midi_trigger ?? null;
	const hasTrigger = Boolean(midiTrigger && midiTrigger.length > 0);
	const resolvedSelectedDevice = selectedDeviceLocal;
	const isResolvedSelectedDeviceAvailable =
		resolvedSelectedDevice !== null && ports.includes(resolvedSelectedDevice);
	const midiDevicePlaceholder =
		ports.length === 0
			? t("settings.midi.device.noDevices")
			: t("settings.midi.device.select");

	useEffect(() => {
		setSelectedDeviceLocal(selectedDevice);
		selectedDeviceRef.current = selectedDevice;
	}, [selectedDevice]);

	useEffect(() => {
		isBindingRef.current = isBinding;
	}, [isBinding]);

	const refreshPorts = useCallback(async () => {
		try {
			const result = await commands.getMidiPorts();
			if (result.status === "ok") {
				setPorts(result.data);
			} else {
				setPorts([]);
				toast.error(result.error);
			}
		} catch (error) {
			console.error("Failed to load MIDI ports:", error);
			toast.error(String(error));
		}
	}, []);

	const persistMidiSettings = useCallback(
		async (payload: MidiSettingsPayload): Promise<boolean> => {
			setIsSaving(true);

			try {
				const result = await commands.updateMidiSettings(
					payload.enabled,
					payload.deviceName,
					payload.trigger,
				);

				if (result.status === "error") {
					toast.error(String(result.error));
					await refreshSettings();
					return false;
				}

				const currentSettings = useSettingsStore.getState().settings;
				if (currentSettings) {
					setSettings({
						...currentSettings,
						midi_enabled: payload.enabled,
						midi_device_name: payload.deviceName,
						midi_trigger: payload.trigger,
					});
				}

				return true;
			} catch (error) {
				console.error("Failed to update MIDI settings:", error);
				toast.error(String(error));
				await refreshSettings();
				return false;
			} finally {
				setIsSaving(false);
			}
		},
		[refreshSettings, setSettings],
	);

	const stopBindingMode = useCallback(async () => {
		isBindingRef.current = false;
		setIsBinding(false);
		try {
			const result = await commands.setMidiBindingMode(false);
			if (result.status === "error") {
				console.error("Failed to stop MIDI binding mode:", result.error);
			}
		} catch (error) {
			console.error("Failed to stop MIDI binding mode:", error);
		}
	}, []);

	useEffect(() => {
		void refreshPorts();
	}, [refreshPorts]);

	useEffect(() => {
		let unsubscribe: (() => void) | null = null;

		const setup = async () => {
			unsubscribe = await subscribeEvent<number[]>(
				"midi-trigger-bound",
				(event) => {
					if (!isBindingRef.current) {
						return;
					}

					const effectiveDevice = selectedDeviceRef.current;

					if (!effectiveDevice) {
						void stopBindingMode();
						return;
					}

					void (async () => {
						const updated = await persistMidiSettings({
							enabled: true,
							deviceName: effectiveDevice,
							trigger: event.payload,
						});
						await stopBindingMode();
						if (!updated) {
							return;
						}
					})();
				},
			);
		};

		void setup();

		return () => {
			if (unsubscribe) {
				unsubscribe();
			}
		};
	}, [persistMidiSettings, stopBindingMode]);

	useEffect(() => {
		return () => {
			void (async () => {
				try {
					const result = await commands.setMidiBindingMode(false);
					if (result.status === "error") {
						console.error(
							"Failed to stop MIDI binding mode on unmount:",
							result.error,
						);
					}
				} catch (error) {
					console.error("Failed to stop MIDI binding mode on unmount:", error);
				}
			})();
		};
	}, []);

	const startBindingMode = async () => {
		if (isSaving) {
			return;
		}

		const effectiveDevice = selectedDeviceRef.current;

		if (!effectiveDevice) {
			return;
		}

		const connected = await persistMidiSettings({
			enabled: true,
			deviceName: effectiveDevice,
			trigger: midiTrigger,
		});
		if (!connected) {
			return;
		}

		isBindingRef.current = true;
		setIsBinding(true);
		try {
			const result = await commands.setMidiBindingMode(true);
			if (result.status === "error") {
				isBindingRef.current = false;
				setIsBinding(false);
				toast.error(String(result.error));
			}
		} catch (error) {
			console.error("Failed to start MIDI binding mode:", error);
			isBindingRef.current = false;
			setIsBinding(false);
			toast.error(String(error));
		}
	};

	const toggleBindingMode = async () => {
		if (isBinding) {
			await stopBindingMode();
			return;
		}

		await startBindingMode();
	};

	const resetDevice = async () => {
		if (isBinding) {
			await stopBindingMode();
		}

		setSelectedDeviceLocal(null);

		await persistMidiSettings({
			enabled: false,
			deviceName: null,
			trigger: midiTrigger,
		});

		selectedDeviceRef.current = null;
	};

	const resetTrigger = async () => {
		if (isBinding) {
			await stopBindingMode();
		}

		await persistMidiSettings({
			enabled: false,
			deviceName: selectedDeviceRef.current,
			trigger: null,
		});
	};

	const triggerSummary = hasTrigger
		? (midiTrigger ?? [])
				.map((byte) => `0x${byte.toString(16).padStart(2, "0").toUpperCase()}`)
				.join(" ")
		: t("settings.midi.trigger.bind");

	const midiOptions = [
		...(resolvedSelectedDevice !== null && !isResolvedSelectedDeviceAvailable
			? [
					{
						value: resolvedSelectedDevice,
						label: resolvedSelectedDevice,
						disabled: true,
					},
				]
			: []),
		...ports.map((port) => ({
			value: port,
			label: port,
		})),
	];

	return (
		<>
			<SettingContainer
				title={t("settings.midi.device.title")}
				description={t("settings.midi.device.description")}
				grouped={true}
			>
				<div className="flex items-center space-x-1">
					<Dropdown
						options={midiOptions}
						selectedValue={resolvedSelectedDevice}
						onSelect={(value) => {
							selectedDeviceRef.current = value;
							setSelectedDeviceLocal(value);
							const shouldEnable = Boolean(value && hasTrigger);
							void persistMidiSettings({
								enabled: shouldEnable,
								deviceName: value,
								trigger: midiTrigger,
							});
						}}
						placeholder={midiDevicePlaceholder}
						onRefresh={refreshPorts}
						disabled={isSaving || ports.length === 0}
					/>
					<ResetButton
						onClick={() => void resetDevice()}
						disabled={isSaving || !selectedDeviceLocal}
						ariaLabel={t("common.reset")}
					/>
				</div>
			</SettingContainer>

			<SettingContainer
				title={t("settings.midi.trigger.title")}
				description={t("settings.midi.trigger.description")}
				grouped={true}
			>
				<div className="flex items-center space-x-1">
					<button
						type="button"
						onClick={() => void toggleBindingMode()}
						disabled={
							isSaving ||
							ports.length === 0 ||
							!isResolvedSelectedDeviceAvailable
						}
						className={`px-2 py-1 text-sm font-semibold rounded-md border transition-colors ${
							isBinding
								? "border-logo-primary bg-logo-primary/30"
								: "bg-mid-gray/10 border-mid-gray/80 hover:bg-logo-primary/10 hover:border-logo-primary"
						}`}
					>
						{isBinding ? t("settings.midi.trigger.binding") : triggerSummary}
					</button>
					<ResetButton
						onClick={() => void resetTrigger()}
						disabled={isSaving || !hasTrigger}
						ariaLabel={t("common.reset")}
					/>
				</div>
			</SettingContainer>
		</>
	);
});
