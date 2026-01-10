import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-shell";
import { commands, AuthUser } from "@/bindings";
import { getOAuthUrl, OAuthProvider } from "@/lib/supabase";

// Decode JWT payload without verification (we trust Supabase's tokens)
function decodeJwtPayload(token: string): Record<string, unknown> | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    const payload = parts[1];
    // Base64url decode
    const base64 = payload.replace(/-/g, "+").replace(/_/g, "/");
    const jsonPayload = decodeURIComponent(
      atob(base64)
        .split("")
        .map((c) => "%" + ("00" + c.charCodeAt(0).toString(16)).slice(-2))
        .join("")
    );
    return JSON.parse(jsonPayload);
  } catch {
    return null;
  }
}

// Token data from implicit OAuth flow
interface ImplicitTokenData {
  access_token: string;
  refresh_token?: string;
  expires_in?: string;
  token_type?: string;
}

interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  user: AuthUser | null;
  error: string | null;
}

export function useAuth() {
  const [state, setState] = useState<AuthState>({
    isAuthenticated: false,
    isLoading: true,
    user: null,
    error: null,
  });

  // Track if we're in the middle of an OAuth flow
  const isAuthenticating = useRef(false);

  // Check initial auth state
  useEffect(() => {
    const checkAuth = async () => {
      try {
        const user = await commands.authGetUser();
        setState({
          isAuthenticated: user.is_authenticated,
          isLoading: false,
          user: user.is_authenticated ? user : null,
          error: null,
        });
      } catch (e) {
        console.error("Failed to check auth state:", e);
        setState({
          isAuthenticated: false,
          isLoading: false,
          user: null,
          error: null,
        });
      }
    };
    checkAuth();
  }, []);

  // Listen for auth callbacks from local server
  useEffect(() => {
    const setupListeners = async () => {
      // Listen for implicit flow tokens (auth://token)
      // Supabase returns tokens in URL fragment (#access_token=...)
      // Our auth server extracts them and POSTs to /auth/token, then emits this event
      const unlistenToken = await listen<string>("auth://token", async (event) => {
        console.log("Received auth://token event, isAuthenticating:", isAuthenticating.current);
        console.log("Token payload length:", event.payload?.length);

        if (!isAuthenticating.current) {
          console.log("Ignoring auth://token - not authenticating");
          return;
        }

        try {
          setState((prev) => ({ ...prev, isLoading: true, error: null }));

          const tokenData: ImplicitTokenData = JSON.parse(event.payload);
          console.log("Parsed implicit flow tokens, has access_token:", !!tokenData.access_token);

          // Decode the JWT to get user info (avoid network request that fails in Tauri)
          const jwtPayload = decodeJwtPayload(tokenData.access_token);
          if (!jwtPayload) {
            throw new Error("Failed to decode access token");
          }

          console.log("Decoded JWT payload:", jwtPayload);

          // Extract user info from JWT payload
          // Supabase JWT contains: sub (user_id), email, user_metadata, app_metadata
          const userId = jwtPayload.sub as string;
          const email = jwtPayload.email as string | undefined;
          const userMetadata = (jwtPayload.user_metadata as Record<string, unknown>) || {};
          const appMetadata = (jwtPayload.app_metadata as Record<string, unknown>) || {};

          if (!userId) {
            throw new Error("No user ID in token");
          }

          // Calculate expires_at from expires_in
          const expiresIn = parseInt(tokenData.expires_in || "3600", 10);
          const expiresAt = Math.floor(Date.now() / 1000) + expiresIn;

          // Save session to Tauri credentials store
          await commands.authSaveSession({
            access_token: tokenData.access_token,
            refresh_token: tokenData.refresh_token || "",
            expires_at: expiresAt,
            user_id: userId,
            email: email ?? null,
            name:
              (userMetadata.full_name as string) ||
              (userMetadata.name as string) ||
              (userMetadata.user_name as string) ||
              null,
            avatar_url:
              (userMetadata.avatar_url as string) ||
              (userMetadata.picture as string) ||
              null,
            provider: (appMetadata.provider as string) || null,
          });

          // Update local state
          const authUser = await commands.authGetUser();
          setState({
            isAuthenticated: true,
            isLoading: false,
            user: authUser,
            error: null,
          });
        } catch (e) {
          console.error("Failed to process implicit flow tokens:", e);
          setState((prev) => ({
            ...prev,
            error: String(e),
            isLoading: false,
          }));
        } finally {
          isAuthenticating.current = false;
          await commands.authStopServer();
        }
      });

      // Also listen for auth errors
      const unlistenError = await listen<string>("auth://error", async (event) => {
        console.error("Auth error from server:", event.payload);
        setState((prev) => ({
          ...prev,
          error: event.payload,
          isLoading: false,
        }));
        isAuthenticating.current = false;
        await commands.authStopServer();
      });

      return () => {
        unlistenToken();
        unlistenError();
      };
    };

    const cleanup = setupListeners();
    return () => {
      cleanup.then((fn) => fn?.());
    };
  }, []);

  // Sign in with OAuth provider
  const signIn = useCallback(async (provider: OAuthProvider) => {
    setState((prev) => ({ ...prev, isLoading: true, error: null }));
    isAuthenticating.current = true;

    try {
      // 1. Start the local callback server and get the callback URL
      const result = await commands.authStartServer();
      if (result.status === "error") {
        throw new Error(result.error);
      }
      const callbackUrl = result.data;

      // 2. Generate OAuth URL with the callback
      const oauthUrl = await getOAuthUrl(provider, callbackUrl);

      // 3. Open OAuth URL in system browser
      await open(oauthUrl);

      // The auth flow will continue when we receive the callback event
    } catch (e) {
      console.error("Failed to start OAuth flow:", e);
      setState((prev) => ({ ...prev, error: String(e), isLoading: false }));
      isAuthenticating.current = false;
      // Clean up server on error
      await commands.authStopServer();
    }
  }, []);

  // Cancel ongoing auth flow
  const cancelAuth = useCallback(async () => {
    isAuthenticating.current = false;
    await commands.authStopServer();
    setState((prev) => ({ ...prev, isLoading: false }));
  }, []);

  // Sign out
  const signOut = useCallback(async () => {
    try {
      await commands.authLogout();
      setState({
        isAuthenticated: false,
        isLoading: false,
        user: null,
        error: null,
      });
    } catch (e) {
      console.error("Failed to sign out:", e);
      setState((prev) => ({ ...prev, error: String(e) }));
    }
  }, []);

  // Refresh user data from store
  const refreshUser = useCallback(async () => {
    try {
      const user = await commands.authGetUser();
      setState((prev) => ({
        ...prev,
        isAuthenticated: user.is_authenticated,
        user: user.is_authenticated ? user : null,
      }));
    } catch (e) {
      console.error("Failed to refresh user:", e);
    }
  }, []);

  return {
    ...state,
    signIn,
    signOut,
    cancelAuth,
    refreshUser,
  };
}
