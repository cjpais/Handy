import { Page } from "@playwright/test";

export interface MockState {
  gmailTasksConnected: boolean;
  calendarConnected: boolean;
  oauthClientConfigured: boolean;
  oauthSuccess: boolean;
  sendSuccess: boolean;
  meetingDetectionEnabled: boolean;
  calendarPromptsEnabled: boolean;
  meetingPromptLeadMinutes: number;
  promptEvents: Array<
    | { action: "start" }
    | { action: "dismiss"; payload: unknown }
    | { action: "close" }
  >;
  lastFollowUp: {
    recipients: string[];
    summary: string;
    actionItems: string[];
  } | null;
  outputLanguage: string;
}

export async function setupMocks(page: Page, initialGoogleConnected = false) {
  page.on("console", (msg) => {
    console.log(`[Browser Console] ${msg.type()}: ${msg.text()}`);
  });
  page.on("pageerror", (err) => {
    console.error(`[Browser PageError]`, err);
  });

  // Inject mock state and Tauri invoke mocks before the page loads.
  await page.addInitScript((connected) => {
    // 1. Initialize Mock State from sessionStorage if available, otherwise default
    const saved = sessionStorage.getItem("__MOCK_STATE__");
    const state = saved
      ? JSON.parse(saved)
      : {
          gmailTasksConnected: connected,
          calendarConnected: false,
          oauthClientConfigured: true,
          oauthSuccess: true,
          sendSuccess: true,
          meetingDetectionEnabled: false,
          calendarPromptsEnabled: false,
          meetingPromptLeadMinutes: 5,
          promptEvents: [],
          lastFollowUp: null,
          outputLanguage: "malayalam",
        };

    // Save/Sync state helper
    const saveState = () => {
      sessionStorage.setItem("__MOCK_STATE__", JSON.stringify(state));
    };

    // Save initial state if not already saved
    if (!saved) {
      saveState();
    }

    (window as any).__MOCK_STATE__ = state;

    // Mock OS Plugin Internals
    (window as any).__TAURI_OS_PLUGIN_INTERNALS__ = {
      platform: "windows",
      eol: "\r\n",
      version: "10.0.0",
      family: "windows",
      os_type: "windows",
      arch: "x86_64",
      exe_extension: "exe",
    };

    // Mock Event Plugin Internals
    (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (event: string, eventId: number) => {
        console.log(
          `[Mock Event Plugin Internals] unregisterListener`,
          event,
          eventId,
        );
      },
    };

    // Initialize Tauri Callbacks and Event Listeners maps
    (window as any).__TAURI_CALLBACKS__ =
      (window as any).__TAURI_CALLBACKS__ || new Map();
    (window as any).__TAURI_EVENT_LISTENERS__ =
      (window as any).__TAURI_EVENT_LISTENERS__ || new Map();

    // Implement transformCallback
    const transformCallback = (cb: any, once?: boolean) => {
      const id = Math.floor(Math.random() * 10000000);
      (window as any).__TAURI_CALLBACKS__.set(id, (payload: any) => {
        if (once) (window as any).__TAURI_CALLBACKS__.delete(id);
        cb(payload);
      });
      return id;
    };

    // Helper to emit events in tests
    (window as any).__EMIT_EVENT__ = (event: string, payload: any) => {
      const listeners = (window as any).__TAURI_EVENT_LISTENERS__;
      if (listeners && listeners.has(event)) {
        const handlerIds = listeners.get(event);
        for (const handlerId of handlerIds) {
          const callback = (window as any).__TAURI_CALLBACKS__.get(handlerId);
          if (callback) {
            callback({ event, payload, id: handlerId });
          }
        }
      }
    };

    // 2. Mock Tauri IPC layer
    (window as any).__TAURI_INTERNALS__ = {
      transformCallback,
      invoke: async (cmd: string, args?: any) => {
        console.log(`[Mock IPC invoke] ${cmd}`, args);
        const state = (window as any).__MOCK_STATE__;

        // Mock event listening command
        if (cmd === "plugin:event|listen") {
          const { event, handler } = args;
          const listeners = (window as any).__TAURI_EVENT_LISTENERS__;
          if (!listeners.has(event)) {
            listeners.set(event, []);
          }
          listeners.get(event).push(handler);
          return handler;
        }

        // Mock permission endpoints
        if (
          cmd.includes("check_microphone_permission") ||
          cmd.includes("checkMicrophonePermission")
        ) {
          return true;
        }
        if (
          cmd.includes("check_accessibility_permission") ||
          cmd.includes("checkAccessibilityPermission")
        ) {
          return true;
        }
        if (cmd === "get_windows_microphone_permission_status") {
          return {
            supported: false,
            overall_access: "allowed",
            device_access: "allowed",
            app_access: "allowed",
            desktop_app_access: "allowed",
          };
        }

        // Mock models check & listing
        if (cmd === "has_any_models_available") {
          return true;
        }
        if (cmd === "get_available_models") {
          return [
            {
              id: "small",
              name: "Small Model",
              description: "A small model",
              filename: "small.bin",
              url: null,
              sha256: null,
              size_mb: 250,
              is_downloaded: true,
              is_downloading: false,
              partial_size: 0,
              is_directory: false,
              engine_type: "whisper",
              accuracy_score: 80,
              speed_score: 90,
              supports_translation: true,
              is_recommended: true,
              supported_languages: ["en", "ml"],
              supports_language_selection: true,
              is_custom: false,
            },
          ];
        }
        if (cmd === "get_current_model") {
          return "small";
        }
        if (cmd === "get_model_info") {
          return {
            id: "small",
            name: "Small Model",
            description: "A small model",
            filename: "small.bin",
            url: null,
            sha256: null,
            size_mb: 250,
            is_downloaded: true,
            is_downloading: false,
            partial_size: 0,
            is_directory: false,
            engine_type: "whisper",
            accuracy_score: 80,
            speed_score: 90,
            supports_translation: true,
            is_recommended: true,
            supported_languages: ["en", "ml"],
            supports_language_selection: true,
            is_custom: false,
          };
        }

        // Mock App settings (unwrapped)
        if (cmd === "get_app_settings") {
          return {
            bindings: {
              transcribe: {
                id: "transcribe",
                name: "Transcribe",
                default_binding: "ctrl+shift+space",
                current_binding: "ctrl+shift+space",
                description: "Trigger transcription",
              },
              cancel: {
                id: "cancel",
                name: "Cancel",
                default_binding: "escape",
                current_binding: "escape",
                description: "Cancel transcription",
              },
              transcribe_with_post_process: {
                id: "transcribe_with_post_process",
                name: "Transcribe with Post-Process",
                default_binding: "ctrl+shift+p",
                current_binding: "ctrl+shift+p",
                description: "Trigger transcription with post-process",
              },
            },
            push_to_talk: false,
            audio_feedback: false,
            audio_feedback_volume: 50,
            sound_theme: "marimba",
            start_hidden: false,
            autostart_enabled: false,
            update_checks_enabled: false,
            selected_model: "small",
            always_on_microphone: false,
            selected_microphone: "Default",
            clamshell_microphone: "Default",
            selected_output_device: "Default",
            translate_to_english: false,
            selected_language: "auto",
            overlay_position: "bottom",
            debug_mode: false,
            log_level: "info",
            custom_words: [],
            model_unload_timeout: "never",
            word_correction_threshold: 0.5,
            history_limit: 100,
            recording_retention_period: "never",
            paste_method: "ctrl_v",
            clipboard_handling: "copy_to_clipboard",
            auto_submit: false,
            auto_submit_key: "enter",
            post_process_enabled: true,
            post_process_provider_id: "google",
            post_process_providers: [
              {
                id: "google",
                label: "Google (Gemini)",
                base_url:
                  "https://generativelanguage.googleapis.com/v1beta/openai",
                requires_api_key: true,
                models: [
                  "gemma-4-26b-a4b-it",
                  "gemini-2.5-flash",
                  "gemini-2.5-pro",
                ],
              },
            ],
            post_process_api_keys: {},
            post_process_models: {},
            post_process_prompts: [
              {
                id: "default_meeting_notes_with_actions",
                name: "Default Meeting Summary",
                prompt: "Generate summary and action items.",
              },
            ],
            post_process_selected_prompt_id:
              "default_meeting_notes_with_actions",
            mute_while_recording: false,
            append_trailing_space: false,
            app_language: "en",
            experimental_enabled: false,
            lazy_stream_close: false,
            keyboard_implementation: "tauri",
            show_tray_icon: true,
            paste_delay_ms: 100,
            typing_tool: "auto",
            external_script_path: null,
            custom_filler_words: null,
            whisper_accelerator: "cpu",
            ort_accelerator: "cpu",
            whisper_gpu_device: 0,
            extra_recording_buffer_ms: 0,
            output_language: state.outputLanguage,
            google_oauth_token: state.gmailTasksConnected
              ? "mock-refresh-token"
              : null,
            google_auth_tokens: {
              gmail_tasks_refresh_token: state.gmailTasksConnected
                ? "mock-refresh-token"
                : null,
              calendar_refresh_token: state.calendarConnected
                ? "mock-calendar-token"
                : null,
            },
            meeting_detection_enabled: state.meetingDetectionEnabled,
            meeting_calendar_prompts_enabled: state.calendarPromptsEnabled,
            meeting_prompt_lead_minutes: state.meetingPromptLeadMinutes,
          };
        }

        if (cmd === "change_output_language_setting") {
          state.outputLanguage = args.language;
          saveState();
          return null;
        }

        // Mock sound themes / audio checks
        if (cmd === "check_custom_sounds" || cmd === "checkCustomSounds") {
          return { start: false, stop: false };
        }

        // Mock history retrieval (pre-populated with a meeting entry)
        if (cmd === "get_history_entries") {
          return {
            entries: [
              {
                id: 1,
                file_name: "meeting_1.wav",
                timestamp: Date.now() - 60000,
                saved: true,
                title: "Project Kickoff",
                transcription_text: "We need to build a speech to text app.",
                post_processed_text: JSON.stringify({
                  summary: "Project kickoff meeting to discuss architecture.",
                  action_items: ["Build tests first", "Verify and document"],
                }),
                post_process_prompt: "default_meeting_notes_with_actions",
                post_process_requested: true,
              },
            ],
            has_more: false,
          };
        }

        // Mock audio path mapping
        if (cmd === "get_audio_file_path" || cmd === "getAudioFilePath") {
          return "meeting_1.wav";
        }

        // Mock device list mapping
        if (cmd === "get_available_microphones") {
          return [{ index: "default", name: "Default", is_default: true }];
        }
        if (cmd === "get_available_output_devices") {
          return [{ index: "default", name: "Default", is_default: true }];
        }

        // Mock app control / initialization
        if (
          cmd === "initialize_enigo" ||
          cmd === "initialize_shortcuts" ||
          cmd === "show_main_window_command"
        ) {
          return null;
        }

        // Google Authentication endpoints
        if (cmd === "get_google_integration_status") {
          return {
            oauth_client_configured: state.oauthClientConfigured,
            gmail_tasks_connected: state.gmailTasksConnected,
            calendar_connected: state.calendarConnected,
            gmail_tasks_available: state.oauthClientConfigured,
            calendar_available: state.oauthClientConfigured,
            meeting_calendar_prompts_enabled: state.calendarPromptsEnabled,
            meeting_detection_enabled: state.meetingDetectionEnabled,
            meeting_prompt_lead_minutes: state.meetingPromptLeadMinutes,
          };
        }

        if (cmd === "connect_google_features") {
          if (state.oauthSuccess) {
            const features = args?.features || [];
            if (features.includes("gmail_tasks"))
              state.gmailTasksConnected = true;
            if (features.includes("calendar")) state.calendarConnected = true;
            saveState();
            return "success";
          } else {
            throw new Error("OAuth flow failed");
          }
        }

        if (cmd === "disconnect_google_feature") {
          if (args?.feature === "gmail_tasks")
            state.gmailTasksConnected = false;
          if (args?.feature === "calendar") state.calendarConnected = false;
          saveState();
          return null;
        }

        if (cmd === "set_meeting_calendar_prompts_enabled") {
          state.calendarPromptsEnabled = !!args?.enabled;
          saveState();
          return null;
        }

        if (cmd === "change_meeting_detection_enabled_setting") {
          state.meetingDetectionEnabled = !!args?.enabled;
          saveState();
          return null;
        }

        if (cmd === "change_meeting_prompt_lead_minutes_setting") {
          state.meetingPromptLeadMinutes =
            args?.minutes ?? args?.leadMinutes ?? 5;
          saveState();
          return null;
        }

        if (cmd === "start_meeting_recording_from_prompt") {
          state.promptEvents.push({ action: "start" });
          saveState();
          return null;
        }

        if (cmd === "dismiss_meeting_prompt") {
          state.promptEvents.push({
            action: "dismiss",
            payload: args?.payload,
          });
          saveState();
          return null;
        }

        if (cmd === "close_meeting_prompt") {
          state.promptEvents.push({ action: "close" });
          saveState();
          return null;
        }

        // Send follow-up email/tasks endpoint
        if (cmd === "send_meeting_follow_up") {
          state.lastFollowUp = {
            recipients: args?.recipients || [],
            summary: args?.summary || "",
            actionItems: args?.action_items || args?.actionItems || [],
          };
          saveState();
          if (state.sendSuccess) {
            return { status: "ok", data: null };
          } else {
            throw "Failed to send follow-up email/tasks";
          }
        }

        // Fallback or unmocked commands
        return null;
      },
      plugins: {},
      convertFileSrc: (src: string) => src,
    };
  }, initialGoogleConnected);
}

export async function getMockState(page: Page): Promise<MockState> {
  return page.evaluate(() => (window as any).__MOCK_STATE__);
}

export async function setMockState(
  page: Page,
  stateUpdates: Partial<MockState>,
): Promise<void> {
  await page.evaluate((updates) => {
    Object.assign((window as any).__MOCK_STATE__, updates);
    sessionStorage.setItem(
      "__MOCK_STATE__",
      JSON.stringify((window as any).__MOCK_STATE__),
    );
  }, stateUpdates);
}
