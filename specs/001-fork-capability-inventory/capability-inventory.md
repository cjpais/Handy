# Fork Capability Inventory (Handy fork)

## Scope

- Settings window
- Onboarding flow
- Recording overlay
- Tray menu
- Platform permission prompts

## Conventions (Phase 2)

- Item id naming: Use lowercase dot-separated ids in the form `<category>.<area>.<item>` (example: `settings.audio_feedback.toggle`).
- Category rules: One of `route`, `component`, `menu`, `settings`, `overlay`, `permission`.
- Decision/rationale rules: `keep` for in-scope features, `hide` for disabled but retained code, `remove` for deletion candidates; default to `keep` if uncertain and note why.
- Missing-data notation: Use `notes` with `missing:commands`, `missing:events`, and/or `missing:settings` when dependency data is unknown.
- Guard point template: id `guard.<area>`; description states scope boundary; covers list contains item ids.

## Inventory Table

| Category | Item | Location | Commands | Events | Settings Keys | Decision | Rationale | Guard Point | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| route | App Shell | src/App.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.navigation | missing:commands;missing:events;missing:settings |
| component | Sidebar | src/components/Sidebar.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.navigation | missing:commands;missing:events;missing:settings |
| settings | AboutSettings | src/components/settings/about/AboutSettings.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | AdvancedSettings | src/components/settings/advanced/AdvancedSettings.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | AlwaysOnMicrophone | src/components/settings/AlwaysOnMicrophone.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | AppDataDirectory | src/components/settings/AppDataDirectory.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | AppLanguageSelector | src/components/settings/AppLanguageSelector.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | AppendTrailingSpace | src/components/settings/AppendTrailingSpace.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | AudioFeedback | src/components/settings/AudioFeedback.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | AutostartToggle | src/components/settings/AutostartToggle.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | BaseUrlField | src/components/settings/PostProcessingSettingsApi/BaseUrlField.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ClipboardHandling | src/components/settings/ClipboardHandling.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ClamshellMicrophoneSelector | src/components/settings/ClamshellMicrophoneSelector.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | CustomWords | src/components/settings/CustomWords.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | DebugPaths | src/components/settings/debug/DebugPaths.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | DebugSettings | src/components/settings/debug/DebugSettings.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ExperimentalToggle | src/components/settings/ExperimentalToggle.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | GeneralSettings | src/components/settings/general/GeneralSettings.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | GlobalShortcutInput | src/components/settings/GlobalShortcutInput.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | HandyKeysShortcutInput | src/components/settings/HandyKeysShortcutInput.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | HistoryLimit | src/components/settings/HistoryLimit.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | HistorySettings | src/components/settings/history/HistorySettings.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | KeyboardImplementationSelector | src/components/settings/debug/KeyboardImplementationSelector.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | LanguageSelector | src/components/settings/LanguageSelector.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | LogDirectory | src/components/settings/debug/LogDirectory.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | LogLevelSelector | src/components/settings/debug/LogLevelSelector.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | MicrophoneSelector | src/components/settings/MicrophoneSelector.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ModelSelect | src/components/settings/PostProcessingSettingsApi/ModelSelect.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ModelUnloadTimeout | src/components/settings/ModelUnloadTimeout.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | MuteWhileRecording | src/components/settings/MuteWhileRecording.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | OutputDeviceSelector | src/components/settings/OutputDeviceSelector.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | PasteDelay | src/components/settings/debug/PasteDelay.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | PasteMethod | src/components/settings/PasteMethod.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | PostProcessingSettings | src/components/settings/post-processing/PostProcessingSettings.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | PostProcessingSettingsApi | src/components/settings/PostProcessingSettingsApi/index.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | PostProcessingSettingsPrompts | src/components/settings/PostProcessingSettingsPrompts.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | PostProcessingToggle | src/components/settings/PostProcessingToggle.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ProviderSelect | src/components/settings/PostProcessingSettingsApi/ProviderSelect.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | PushToTalk | src/components/settings/PushToTalk.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | RecordingRetentionPeriod | src/components/settings/RecordingRetentionPeriod.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ShortcutInput | src/components/settings/ShortcutInput.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ShowOverlay | src/components/settings/ShowOverlay.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | SoundPicker | src/components/settings/SoundPicker.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | StartHidden | src/components/settings/StartHidden.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | TranslateToEnglish | src/components/settings/TranslateToEnglish.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | UpdateChecksToggle | src/components/settings/UpdateChecksToggle.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | VolumeSlider | src/components/settings/VolumeSlider.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | WordCorrectionThreshold | src/components/settings/debug/WordCorrectionThreshold.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| settings | ApiKeyField | src/components/settings/PostProcessingSettingsApi/ApiKeyField.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.settings | missing:commands;missing:events;missing:settings |
| component | Onboarding | src/components/onboarding/Onboarding.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.onboarding | missing:commands;missing:events;missing:settings |
| component | AccessibilityOnboarding | src/components/onboarding/AccessibilityOnboarding.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.onboarding | missing:commands;missing:events;missing:settings |
| component | ModelCard | src/components/onboarding/ModelCard.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.onboarding | missing:commands;missing:events;missing:settings |
| overlay | RecordingOverlay | src/overlay/RecordingOverlay.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.overlay | missing:commands;missing:events;missing:settings |
| overlay | OverlayBootstrap | src/overlay/main.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.overlay | missing:commands;missing:events;missing:settings |
| overlay | OverlayShell | src/overlay/index.html | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.overlay | missing:commands;missing:events;missing:settings |
| menu | Tray Menu | src-tauri/src/tray.rs | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.tray | missing:commands;missing:events;missing:settings |
| menu | Tray Labels | src-tauri/src/tray_i18n.rs | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.tray | missing:commands;missing:events;missing:settings |
| permission | AccessibilityPermissions | src/components/AccessibilityPermissions.tsx | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.permissions | missing:commands;missing:events;missing:settings |
| permission | PermissionPromptsBackend | src-tauri/src (permissions-related modules) | Unknown | Unknown | Unknown | keep | Default keep pending scope decision | guard.permissions | missing:commands;missing:events;missing:settings |

## Guard Points

| Guard Point | Description | Covers |
| --- | --- | --- |
| guard.settings | Disable all settings panels | All settings items |
| guard.onboarding | Disable onboarding flow | All onboarding items |
| guard.navigation | Disable main navigation shell | App Shell, Sidebar |
| guard.overlay | Disable recording overlay UI | Overlay items |
| guard.tray | Disable tray menu items | Tray Menu, Tray Labels |
| guard.permissions | Disable permission prompts | Permission items |

## Cross-Check Notes

- README reviewed for additional user-visible surfaces; no extra UI areas identified beyond current scope.
- External architecture references were not cross-checked because they are not part of the repository.

## JSON Parity Notes

- capabilities.json mirrors the inventory list using normalized item ids derived from the table entries.
