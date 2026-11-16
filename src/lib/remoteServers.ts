import type {
  AppSettings,
  RemoteAsrServer,
  RemoteBody,
  RequestPair,
} from "@/bindings";

/** A server with every field present (the stored form may omit defaults). */
export type RemoteServer = Required<RemoteAsrServer>;

export const withDefaults = (server: RemoteAsrServer): RemoteServer => ({
  ...server,
  id: server.id ?? "",
  name: server.name ?? "",
  url: server.url ?? "",
  api_key: server.api_key ?? "",
  has_api_key: server.has_api_key ?? false,
  headers: server.headers ?? [],
  body: server.body ?? "multipart",
  fields: server.fields ?? [],
  prompt: server.prompt ?? "",
  json_body: server.json_body ?? "",
  file_field: server.file_field ?? "file",
  text_paths: server.text_paths ?? "text",
  timeout_secs: server.timeout_secs ?? 120,
});

/**
 * Starting points for a new server. They only fill the form: every field
 * stays editable, and the backend knows nothing about any of them.
 */
export interface RemoteServerPreset {
  key: string;
  name: string;
  url: string;
  body: RemoteBody;
  headers: RequestPair[];
  fields: RequestPair[];
  prompt: string;
  json_body: string;
  file_field: string;
  text_paths: string;
}

const BEARER = [{ name: "Authorization", value: "Bearer {api_key}" }];

const multipart = (
  preset: Omit<
    RemoteServerPreset,
    "body" | "prompt" | "json_body" | "file_field"
  >,
): RemoteServerPreset => ({
  ...preset,
  body: "multipart",
  prompt: "",
  json_body: "",
  file_field: "file",
});

/**
 * What a chat request tells an audio LLM. As the system message with the
 * audio alone in the user turn: an instruction next to the audio tends to get
 * answered instead of transcribed.
 */
export const DEFAULT_CHAT_PROMPT =
  "You are a speech-to-text engine. Write down exactly what is said in the audio, in the language it is spoken. Reply with the transcript alone, without quotation marks, labels or commentary.";

/** Where an OpenAI-style chat reply keeps its text. */
export const CHAT_TEXT_PATH = "choices.0.message.content";

/** A starting point for a hand-written JSON body: the chat request, spelled out. */
export const JSON_BODY_STARTER = JSON.stringify(
  {
    messages: [
      { role: "system", content: DEFAULT_CHAT_PROMPT },
      {
        role: "user",
        content: [
          {
            type: "input_audio",
            input_audio: { data: "{audio_base64}", format: "wav" },
          },
        ],
      },
    ],
    temperature: 0,
  },
  null,
  2,
);

export const REMOTE_SERVER_PRESETS: RemoteServerPreset[] = [
  multipart({
    key: "openthai",
    name: "OpenThai2.0 ASR",
    url: "http://localhost:18081/v1/transcribe",
    headers: BEARER,
    fields: [
      { name: "lang", value: "th-mix" },
      { name: "mode", value: "standard" },
    ],
    text_paths: "display_text, text",
  }),
  multipart({
    key: "whisper-cpp",
    name: "whisper.cpp server",
    url: "http://localhost:8080/inference",
    headers: [],
    fields: [
      // whisper.cpp falls back to English, not detection, without a language.
      { name: "language", value: "{language|auto}" },
      { name: "response_format", value: "json" },
    ],
    text_paths: "text",
  }),
  {
    key: "llama-cpp",
    name: "llama.cpp audio model",
    url: "http://localhost:8080/v1/chat/completions",
    body: "chat",
    headers: BEARER,
    fields: [
      { name: "model", value: "" },
      { name: "temperature", value: "0" },
    ],
    prompt: DEFAULT_CHAT_PROMPT,
    json_body: "",
    file_field: "file",
    text_paths: CHAT_TEXT_PATH,
  },
  multipart({
    key: "mlx-audio",
    name: "MLX Audio",
    url: "http://localhost:8000/v1/audio/transcriptions",
    headers: [],
    fields: [
      { name: "model", value: "mlx-community/whisper-large-v3-turbo-asr-fp16" },
      { name: "language", value: "{language}" },
      // mlx-audio streams NDJSON unless asked for JSON.
      { name: "response_format", value: "json" },
    ],
    text_paths: "text",
  }),
  multipart({
    key: "openai-compatible",
    name: "OpenAI-compatible server",
    url: "http://localhost:8000/v1/audio/transcriptions",
    headers: BEARER,
    fields: [
      { name: "model", value: "" },
      { name: "language", value: "{language}" },
      { name: "response_format", value: "json" },
    ],
    text_paths: "text",
  }),
  multipart({
    key: "custom",
    name: "",
    url: "",
    headers: BEARER,
    fields: [],
    text_paths: "text",
  }),
];

export const newServerFromPreset = (
  preset: RemoteServerPreset,
  fallbackName: string,
): RemoteServer => ({
  id: crypto.randomUUID(),
  name: preset.name || fallbackName,
  url: preset.url,
  body: preset.body,
  prompt: preset.prompt,
  json_body: preset.json_body,
  api_key: "",
  has_api_key: false,
  headers: preset.headers.map((p) => ({ ...p })),
  fields: preset.fields.map((p) => ({ ...p })),
  file_field: preset.file_field,
  text_paths: preset.text_paths,
  timeout_secs: 120,
});

/** The server dictation uses, mirroring the backend's `active_remote_server`. */
export const activeRemoteServer = (
  settings: Partial<AppSettings> | null | undefined,
): RemoteAsrServer | null => {
  if (!settings?.remote_transcription_enabled) return null;
  const id = settings.remote_transcription_server_id;
  return (
    settings.remote_transcription_servers?.find(
      (s) => s.id === id && (s.url ?? "").trim() !== "",
    ) ?? null
  );
};

/**
 * What changes with the way audio is sent: the chat and JSON modes get a
 * prompt or a body to start from, and the reply's text path follows the mode
 * unless the user set their own.
 */
export const withBody = (
  server: RemoteServer,
  body: RemoteBody,
): RemoteServer => {
  const chatLike = (b: RemoteBody) => b === "chat" || b === "json";
  const next = { ...server, body };
  if (body === "chat" && !server.prompt.trim())
    next.prompt = DEFAULT_CHAT_PROMPT;
  if (body === "json" && !server.json_body.trim())
    next.json_body = JSON_BODY_STARTER;
  if (chatLike(body) && server.text_paths.trim() === "text")
    next.text_paths = CHAT_TEXT_PATH;
  if (!chatLike(body) && server.text_paths.trim() === CHAT_TEXT_PATH)
    next.text_paths = "text";
  return next;
};

/** Whether `host` is this machine or on a private network. */
const isPrivateHost = (host: string): boolean => {
  if (/^\d+\.\d+\.\d+\.\d+$/.test(host)) {
    const [a, b] = host.split(".").map(Number);
    return (
      a === 127 ||
      a === 10 ||
      (a === 192 && b === 168) ||
      (a === 169 && b === 254) ||
      (a === 172 && b >= 16 && b <= 31) ||
      // Carrier-grade NAT, which Tailscale uses.
      (a === 100 && b >= 64 && b <= 127)
    );
  }
  if (host.includes(":")) {
    // IPv6: loopback, unique local (fc00::/7), link-local (fe80::/10).
    return (
      host === "::1" ||
      /^f[cd][0-9a-f]{2}:/i.test(host) ||
      /^fe[89ab][0-9a-f]:/i.test(host)
    );
  }
  return (
    host === "localhost" ||
    !host.includes(".") ||
    [".local", ".lan", ".internal", ".home.arpa"].some((suffix) =>
      host.endsWith(suffix),
    )
  );
};

/**
 * Whether `url` sends recordings (and any key) unencrypted to a machine that
 * isn't this one or on the local network.
 */
export const isUnencryptedOverInternet = (url: string): boolean => {
  let parsed: URL;
  try {
    parsed = new URL(url.trim());
  } catch {
    return false;
  }
  if (parsed.protocol !== "http:") return false;
  return !isPrivateHost(parsed.hostname.replace(/^\[|\]$/g, "").toLowerCase());
};
