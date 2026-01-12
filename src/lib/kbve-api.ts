import { commands } from "@/bindings";

// KBVE API base URL - can be configured via environment or hardcoded
const KBVE_API_URL = "https://kbve.com"; // or your staging URL

/**
 * User profile response from KBVE API
 */
export interface KBVEProfile {
  user_id: string;
  username?: string;
  email?: string;
  role?: string;
  profile_exists?: boolean;
  discord?: {
    id: string;
    username: string;
    avatar_url?: string;
    is_guild_member?: boolean;
    guild_nickname?: string;
    joined_at?: string;
    role_ids?: string[];
    role_names?: string[];
    is_boosting?: boolean;
  };
  github?: {
    id: string;
    username: string;
    avatar_url?: string;
  };
  twitch?: {
    id: string;
    username: string;
    avatar_url?: string;
    is_live?: boolean;
  };
  rentearth?: Record<string, unknown>;
  connected_providers?: string[];
  provider_count?: number;
}

/**
 * Fetch the current user's profile from KBVE API
 * Uses the stored access token for authentication
 */
export async function fetchMyProfile(): Promise<KBVEProfile> {
  // Get the stored access token from Tauri credentials store
  const accessToken = await commands.authGetAccessToken();

  if (!accessToken) {
    throw new Error("Not authenticated or token expired");
  }

  const response = await fetch(`${KBVE_API_URL}/api/v1/profile/me`, {
    method: "GET",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
  });

  if (!response.ok) {
    if (response.status === 401) {
      throw new Error("Authentication expired. Please sign in again.");
    }
    if (response.status === 503) {
      throw new Error("API service unavailable. Please try again later.");
    }
    throw new Error(`API error: ${response.status} ${response.statusText}`);
  }

  return response.json();
}

/**
 * Fetch a public user profile by username
 */
export async function fetchProfileByUsername(
  username: string
): Promise<KBVEProfile> {
  const response = await fetch(
    `${KBVE_API_URL}/api/v1/profile/${encodeURIComponent(username)}`,
    {
      method: "GET",
      headers: {
        "Content-Type": "application/json",
      },
    }
  );

  if (!response.ok) {
    if (response.status === 404) {
      throw new Error("Profile not found");
    }
    throw new Error(`API error: ${response.status} ${response.statusText}`);
  }

  return response.json();
}

/**
 * Username validation error types
 */
export type UsernameError =
  | "too_short"
  | "too_long"
  | "invalid_characters"
  | "must_start_with_letter"
  | "empty";

/**
 * Username validation result
 */
export interface UsernameValidationResult {
  valid: boolean;
  error?: UsernameError;
  message?: string;
  normalized?: string;
}

/**
 * Validate username format (client-side belt in belt-and-suspenders)
 * Rules: 3-24 characters, alphanumeric + underscore, must start with letter
 */
export function validateUsername(username: string): UsernameValidationResult {
  const trimmed = username.trim();

  if (!trimmed) {
    return { valid: false, error: "empty", message: "Username cannot be empty" };
  }

  if (trimmed.length < 3) {
    return {
      valid: false,
      error: "too_short",
      message: "Username must be at least 3 characters",
    };
  }

  if (trimmed.length > 24) {
    return {
      valid: false,
      error: "too_long",
      message: "Username must be at most 24 characters",
    };
  }

  // Check first character is a letter
  if (!/^[a-zA-Z]/.test(trimmed)) {
    return {
      valid: false,
      error: "must_start_with_letter",
      message: "Username must start with a letter",
    };
  }

  // Full validation: only letters, numbers, and underscores
  if (!/^[a-zA-Z][a-zA-Z0-9_]{2,23}$/.test(trimmed)) {
    return {
      valid: false,
      error: "invalid_characters",
      message: "Username can only contain letters, numbers, and underscores",
    };
  }

  return { valid: true, normalized: trimmed.toLowerCase() };
}

/**
 * Response from set username endpoint
 */
export interface SetUsernameResponse {
  success: boolean;
  username?: string;
  message?: string;
  error?: string;
}

/**
 * Set username for the authenticated user
 * Validates client-side first, then sends to API
 *
 * @param username - Desired username (3-24 chars, alphanumeric + underscore, starts with letter)
 * @returns The canonical (lowercased) username on success
 * @throws Error with descriptive message on failure
 */
export async function setUsername(username: string): Promise<string> {
  // Client-side validation (belt)
  const validation = validateUsername(username);
  if (!validation.valid) {
    throw new Error(validation.message || "Invalid username");
  }

  // Get access token
  const accessToken = await commands.authGetAccessToken();
  if (!accessToken) {
    throw new Error("Not authenticated or token expired");
  }

  // Call API (suspenders validation happens server-side)
  const response = await fetch(`${KBVE_API_URL}/api/v1/profile/username`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ username: validation.normalized }),
  });

  const data: SetUsernameResponse = await response.json();

  if (!response.ok) {
    // Handle specific error cases
    if (response.status === 401) {
      throw new Error("Authentication expired. Please sign in again.");
    }
    if (response.status === 409) {
      throw new Error(data.error || "Username already taken");
    }
    if (response.status === 400) {
      throw new Error(data.error || "Invalid username format");
    }
    if (response.status === 503) {
      throw new Error("API service unavailable. Please try again later.");
    }
    throw new Error(data.error || `API error: ${response.status}`);
  }

  if (!data.success || !data.username) {
    throw new Error(data.error || "Failed to set username");
  }

  return data.username;
}

/**
 * Check API health
 */
export async function checkApiHealth(): Promise<boolean> {
  try {
    const response = await fetch(`${KBVE_API_URL}/health`, {
      method: "GET",
    });
    return response.ok;
  } catch {
    return false;
  }
}
