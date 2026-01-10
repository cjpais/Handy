import { createClient, SupabaseClient } from "@supabase/supabase-js";
import { commands } from "@/bindings";

// Singleton Supabase client
let supabaseClient: SupabaseClient | null = null;
let clientInitPromise: Promise<SupabaseClient> | null = null;

/**
 * Gets or creates a singleton Supabase client configured for Tauri desktop app.
 * Uses credentials stored in the Tauri credentials store.
 */
export async function getSupabaseClient(): Promise<SupabaseClient> {
  if (supabaseClient) {
    return supabaseClient;
  }

  // Prevent multiple simultaneous initializations
  if (clientInitPromise) {
    return clientInitPromise;
  }

  clientInitPromise = (async () => {
    // Get Supabase credentials from Tauri store
    const supabaseUrl = await commands.getSupabaseUrl();
    const supabaseAnonKey = await commands.getSupabaseAnonKeyRaw();

    supabaseClient = createClient(supabaseUrl, supabaseAnonKey, {
      auth: {
        // Use implicit flow - tokens returned in URL fragment (#access_token=)
        // Our auth server handles extracting tokens from fragment and POSTing them
        flowType: "implicit",
        // Disable auto-refresh since we handle sessions via Tauri store
        autoRefreshToken: false,
        // We store sessions in Tauri's secure credential store, not browser storage
        persistSession: false,
        // We handle the OAuth callback via our local server
        detectSessionInUrl: false,
      },
    });

    return supabaseClient;
  })();

  return clientInitPromise;
}

/**
 * @deprecated Use getSupabaseClient() instead
 */
export async function createSupabaseClient() {
  return getSupabaseClient();
}

/**
 * OAuth provider types supported by our auth flow
 */
export type OAuthProvider = "github" | "discord" | "twitch";

/**
 * Generate the OAuth URL for a given provider.
 * The callback URL points to our local auth server on port 4321.
 */
export async function getOAuthUrl(
  provider: OAuthProvider,
  callbackUrl: string
): Promise<string> {
  const supabase = await getSupabaseClient();

  const { data, error } = await supabase.auth.signInWithOAuth({
    provider,
    options: {
      redirectTo: callbackUrl,
      skipBrowserRedirect: true, // We'll open the URL manually
    },
  });

  if (error) {
    throw new Error(`Failed to generate OAuth URL: ${error.message}`);
  }

  return data.url;
}

