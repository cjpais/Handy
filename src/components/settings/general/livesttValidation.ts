const LIVESTT_SERVER_URL_ERROR_KEY =
  "settings.transcriptionBackend.livestt.serverUrl.error";

export const MIN_FINALIZE_TIMEOUT_MS = 1;
export const MAX_FINALIZE_TIMEOUT_MS = 120000;
export const MAX_LIVESTT_PROMPT_CHARS = 10000;

export interface LiveSttServerUrlValidationResult {
  normalized: string;
  isEmpty: boolean;
  isValid: boolean;
}

const isAllowedLiveSttHttpHost = (host: string): boolean => {
  const normalizedHost = host.toLowerCase();
  return (
    normalizedHost === "localhost" ||
    normalizedHost === "127.0.0.1" ||
    normalizedHost === "::1" ||
    normalizedHost === "[::1]"
  );
};

export const normalizeLiveSttServerUrlInput = (
  input: string,
): LiveSttServerUrlValidationResult => {
  const trimmed = input.trim();
  if (!trimmed) {
    return { normalized: "", isEmpty: true, isValid: true };
  }

  try {
    const url = new URL(trimmed);

    if (url.protocol === "http:") {
      if (!isAllowedLiveSttHttpHost(url.hostname)) {
        return { normalized: "", isEmpty: false, isValid: false };
      }
    } else if (url.protocol !== "https:") {
      return { normalized: "", isEmpty: false, isValid: false };
    }

    if ((url.pathname && url.pathname !== "/") || url.search || url.hash) {
      return { normalized: "", isEmpty: false, isValid: false };
    }

    return { normalized: url.origin, isEmpty: false, isValid: true };
  } catch {
    return { normalized: "", isEmpty: false, isValid: false };
  }
};

export const validateLiveSttServerUrlInput = (value: string): string | null => {
  return normalizeLiveSttServerUrlInput(value).isValid
    ? null
    : LIVESTT_SERVER_URL_ERROR_KEY;
};

export const isLiveSttServerUrlValidForLogin = (value: string): boolean => {
  const validation = normalizeLiveSttServerUrlInput(value);
  return !validation.isEmpty && validation.isValid;
};

export const parseConsultationIdInput = (value: string): string | null => {
  const trimmed = value.trim();
  if (trimmed === "") {
    return "";
  }

  const parsed = Number(trimmed);
  if (!/^\d+$/.test(trimmed) || !Number.isSafeInteger(parsed) || parsed < 1) {
    return null;
  }

  return String(parsed);
};

export const normalizeLiveSttPromptInput = (
  value: string,
): { trimmed: string; isValid: boolean } => {
  const trimmed = value.trim();
  if ([...trimmed].length > MAX_LIVESTT_PROMPT_CHARS) {
    return { trimmed, isValid: false };
  }
  return { trimmed, isValid: true };
};

export const validateFinalizeTimeoutInput = (value: string): number | null => {
  const parsed = Number(value.trim());
  if (
    !Number.isInteger(parsed) ||
    parsed < MIN_FINALIZE_TIMEOUT_MS ||
    parsed > MAX_FINALIZE_TIMEOUT_MS
  ) {
    return null;
  }

  return parsed;
};
