import type { SecretMap } from "@/bindings";

export const ELEVENLABS_TRANSCRIPTION_PROVIDER_ID = "elevenlabs";

export interface ExternalTranscriptionModelProfile {
  fullLabel: string;
  description: string;
  accuracyScore: number;
  speedScore: number;
}

const ELEVENLABS_PROFILE: ExternalTranscriptionModelProfile = {
  fullLabel: "Scribe v2 by ElevenLabs",
  description:
    "Highest-accuracy cloud transcription with multilingual support.",
  accuracyScore: 1,
  speedScore: 0.58,
};

export const getElevenLabsModelProfile =
  (): ExternalTranscriptionModelProfile => ELEVENLABS_PROFILE;

export const hasTranscriptionProviderApiKey = (
  providerId: string,
  apiKeys?: SecretMap | null,
): boolean => {
  const apiKey = apiKeys?.[providerId];
  return typeof apiKey === "string" && apiKey.trim().length > 0;
};

export const getActiveTranscriptionModelDisplayName = (
  providerId?: string | null,
): string | null => {
  if (providerId !== ELEVENLABS_TRANSCRIPTION_PROVIDER_ID) {
    return null;
  }

  return ELEVENLABS_PROFILE.fullLabel;
};
