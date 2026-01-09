# Supabase OAuth Integration Plan

Add user authentication via Supabase OAuth, allowing users to log into the KBVE Supabase instance.

## Overview

**Goal**: Enable secure user authentication using Supabase OAuth with support for multiple providers (Google, Discord, GitHub, etc.)

**Architecture**: Local HTTP server for OAuth callback + Supabase JS client on frontend + token storage in credentials store

---

## Architecture

### Authentication Flow (Local Webserver)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    OAuth Flow (PKCE + Local Server)                     │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  1. User clicks "Sign In"                                               │
│           │                                                             │
│           ▼                                                             │
│  2. Backend starts local HTTP server on port 4321                       │
│           │                                                             │
│           ▼                                                             │
│  3. Frontend generates PKCE code_verifier + code_challenge              │
│           │                                                             │
│           ▼                                                             │
│  4. Open browser to Supabase OAuth URL                                  │
│     redirect_to: http://127.0.0.1:4321/auth/callback                    │
│           │                                                             │
│           ▼                                                             │
│  5. User authenticates with provider (Google, Discord, etc.)            │
│           │                                                             │
│           ▼                                                             │
│  6. Supabase redirects to: http://127.0.0.1:4321/auth/callback?code=X   │
│           │                                                             │
│           ▼                                                             │
│  7. Local server catches request, extracts code, shows success page     │
│           │                                                             │
│           ▼                                                             │
│  8. Server emits event to frontend with auth code                       │
│           │                                                             │
│           ▼                                                             │
│  9. Frontend exchanges code + code_verifier for session tokens          │
│           │                                                             │
│           ▼                                                             │
│  10. Store tokens securely in credentials.json                          │
│           │                                                             │
│           ▼                                                             │
│  11. Server shuts down, user is authenticated                           │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### Why Local Webserver?

- **More reliable than deep links** - Works consistently across all platforms
- **No OS registration needed** - Deep links require registry entries (Windows) or Info.plist (macOS)
- **Better Linux support** - Deep links are unreliable on Linux/Wayland
- **Same pattern as Spotify, VSCode, etc.** - Industry standard for desktop OAuth
- **Firewall friendly** - Only binds to localhost (127.0.0.1)

### Callback Configuration

- **Callback URL**: `http://127.0.0.1:4321/auth/callback`
- **Port**: Fixed port 4321 (whitelisted in Supabase)
- **Lifetime**: Server starts on sign-in click, shuts down after receiving callback

---

## Implementation Plan

### Phase 1: Auth Server (Rust Backend)

**New file: `src-tauri/src/auth_server.rs`**

```rust
//! Local OAuth callback server
//!
//! Spawns a temporary HTTP server to receive OAuth callbacks from Supabase.
//! More reliable than deep links, especially on Linux.

use log::{info, error};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Sender, Receiver};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const AUTH_CALLBACK_EVENT: &str = "auth://callback";
const AUTH_SERVER_PORT: u16 = 4321;

/// HTML page shown to user after successful OAuth redirect
const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Authentication Successful</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
        }
        .container {
            text-align: center;
            padding: 40px;
            background: rgba(255,255,255,0.1);
            border-radius: 16px;
            backdrop-filter: blur(10px);
        }
        h1 { margin-bottom: 16px; }
        p { opacity: 0.9; }
    </style>
</head>
<body>
    <div class="container">
        <h1>✓ Authentication Successful</h1>
        <p>You can close this window and return to the app.</p>
    </div>
</body>
</html>"#;

/// HTML page shown on error
const ERROR_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Authentication Failed</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #f44336;
            color: white;
        }
        .container { text-align: center; padding: 40px; }
    </style>
</head>
<body>
    <div class="container">
        <h1>✗ Authentication Failed</h1>
        <p>Please try again from the app.</p>
    </div>
</body>
</html>"#;

pub struct AuthServer {
    port: u16,
    shutdown_tx: Option<Sender<()>>,
}

impl AuthServer {
    /// Start the auth server on the fixed port 4321
    pub fn start(app_handle: AppHandle) -> Result<Self, String> {
        // Bind to fixed port 4321 (whitelisted in Supabase)
        let listener = TcpListener::bind(format!("127.0.0.1:{}", AUTH_SERVER_PORT))
            .map_err(|e| format!("Failed to bind auth server on port {}: {}", AUTH_SERVER_PORT, e))?;

        let port = AUTH_SERVER_PORT;

        // Set non-blocking so we can check for shutdown
        listener.set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        info!("Auth server started on port {}", port);

        // Spawn listener thread
        thread::spawn(move || {
            Self::run_server(listener, app_handle, shutdown_rx);
        });

        Ok(Self {
            port,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Get the callback URL for this server
    pub fn callback_url(&self) -> String {
        format!("http://127.0.0.1:{}/auth/callback", self.port)
    }

    /// Get the port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Shutdown the server
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    fn run_server(listener: TcpListener, app_handle: AppHandle, shutdown_rx: Receiver<()>) {
        loop {
            // Check for shutdown signal
            if shutdown_rx.try_recv().is_ok() {
                info!("Auth server shutting down");
                break;
            }

            // Try to accept a connection (non-blocking)
            match listener.accept() {
                Ok((stream, _addr)) => {
                    if Self::handle_connection(stream, &app_handle) {
                        // Successfully handled auth callback, shutdown
                        info!("Auth callback received, server shutting down");
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection yet, sleep briefly and try again
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    error!("Auth server accept error: {}", e);
                }
            }
        }
    }

    fn handle_connection(mut stream: TcpStream, app_handle: &AppHandle) -> bool {
        let mut buffer = [0; 4096];

        if let Err(e) = stream.read(&mut buffer) {
            error!("Failed to read request: {}", e);
            return false;
        }

        let request = String::from_utf8_lossy(&buffer);

        // Parse the request line
        let first_line = request.lines().next().unwrap_or("");

        // Check if this is the callback path
        if !first_line.contains("/auth/callback") {
            // Not the callback, return 404
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
            return false;
        }

        // Extract query string from GET /auth/callback?code=XXX HTTP/1.1
        let query_start = first_line.find('?');
        let query_end = first_line.rfind(" HTTP");

        let (code, error) = if let (Some(start), Some(end)) = (query_start, query_end) {
            let query = &first_line[start + 1..end];
            Self::parse_query(query)
        } else {
            (None, Some("No query parameters".to_string()))
        };

        // Send response to browser
        let (status, html) = if code.is_some() {
            ("200 OK", SUCCESS_HTML)
        } else {
            ("400 Bad Request", ERROR_HTML)
        };

        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            html.len(),
            html
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();

        // Emit event to frontend
        if let Some(auth_code) = code {
            info!("Auth code received, emitting to frontend");
            let _ = app_handle.emit(AUTH_CALLBACK_EVENT, auth_code);
            true
        } else {
            error!("Auth error: {:?}", error);
            let _ = app_handle.emit("auth://error", error.unwrap_or_default());
            false
        }
    }

    fn parse_query(query: &str) -> (Option<String>, Option<String>) {
        let mut code = None;
        let mut error = None;

        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");

            match key {
                "code" => code = Some(urlencoding::decode(value).unwrap_or_default().into_owned()),
                "error" => error = Some(urlencoding::decode(value).unwrap_or_default().into_owned()),
                "error_description" => {
                    if error.is_none() {
                        error = Some(urlencoding::decode(value).unwrap_or_default().into_owned());
                    }
                }
                _ => {}
            }
        }

        (code, error)
    }
}

impl Drop for AuthServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}
```

**Modify: `src-tauri/Cargo.toml`**
```toml
[dependencies]
urlencoding = "2"
```

### Phase 2: Auth Commands (Rust Backend)

**New file: `src-tauri/src/commands/auth.rs`**

```rust
//! Supabase authentication commands
//!
//! Security: Auth tokens stored in credentials.json, never returned in full.
//! Session refresh handled automatically by frontend Supabase client.

use crate::auth_server::AuthServer;
use log::info;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_store::StoreExt;

const CREDENTIALS_STORE_PATH: &str = "credentials.json";
const SUPABASE_SESSION_KEY: &str = "supabase_session";

/// Manages the temporary auth server
pub struct AuthManager {
    server: Mutex<Option<AuthServer>>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            server: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SupabaseSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user_id: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
    pub is_authenticated: bool,
}

/// Start the OAuth callback server and return the callback URL
/// Call this before opening the OAuth URL in the browser
#[tauri::command]
#[specta::specta]
pub fn auth_start_server(
    app: AppHandle,
    auth_manager: State<'_, Arc<AuthManager>>,
) -> Result<String, String> {
    let mut server_guard = auth_manager.server.lock().unwrap();

    // Shutdown any existing server
    if let Some(mut old_server) = server_guard.take() {
        old_server.shutdown();
    }

    // Start new server
    let server = AuthServer::start(app)?;
    let callback_url = server.callback_url();

    info!("Auth server started, callback URL: {}", callback_url);

    *server_guard = Some(server);
    Ok(callback_url)
}

/// Stop the OAuth callback server (called after auth completes or on cancel)
#[tauri::command]
#[specta::specta]
pub fn auth_stop_server(auth_manager: State<'_, Arc<AuthManager>>) {
    let mut server_guard = auth_manager.server.lock().unwrap();
    if let Some(mut server) = server_guard.take() {
        server.shutdown();
        info!("Auth server stopped");
    }
}

/// Save Supabase session to secure credentials store
#[tauri::command]
#[specta::specta]
pub fn auth_save_session(app: AppHandle, session: SupabaseSession) -> Result<(), String> {
    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.set(SUPABASE_SESSION_KEY, serde_json::to_value(&session).unwrap());
    store.save().map_err(|e| format!("Failed to save session: {}", e))?;

    info!("Supabase session saved for user: {}", session.user_id);
    Ok(())
}

/// Load Supabase session from credentials store
#[tauri::command]
#[specta::specta]
pub fn auth_get_session(app: AppHandle) -> Option<SupabaseSession> {
    let store = app.store(CREDENTIALS_STORE_PATH).ok()?;
    store
        .get(SUPABASE_SESSION_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Get current authenticated user info (safe to expose)
#[tauri::command]
#[specta::specta]
pub fn auth_get_user(app: AppHandle) -> AuthUser {
    match auth_get_session(app) {
        Some(session) => AuthUser {
            id: session.user_id,
            email: session.email,
            is_authenticated: true,
        },
        None => AuthUser {
            id: String::new(),
            email: None,
            is_authenticated: false,
        },
    }
}

/// Clear auth session (logout)
#[tauri::command]
#[specta::specta]
pub fn auth_logout(app: AppHandle) -> Result<(), String> {
    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.delete(SUPABASE_SESSION_KEY);
    store.save().map_err(|e| format!("Failed to save: {}", e))?;

    info!("User logged out, session cleared");
    Ok(())
}

/// Check if user is authenticated
#[tauri::command]
#[specta::specta]
pub fn auth_is_authenticated(app: AppHandle) -> bool {
    auth_get_session(app).is_some()
}
```

**Modify: `src-tauri/src/commands/mod.rs`**
```rust
pub mod auth;
```

**Modify: `src-tauri/src/lib.rs`**
```rust
mod auth_server;

// In run() function, register AuthManager as state:
let auth_manager = Arc::new(commands::auth::AuthManager::new());
app.manage(auth_manager);

// Register commands:
commands::auth::auth_start_server,
commands::auth::auth_stop_server,
commands::auth::auth_save_session,
commands::auth::auth_get_session,
commands::auth::auth_get_user,
commands::auth::auth_logout,
commands::auth::auth_is_authenticated,
```

### Phase 3: Frontend Supabase Integration

**Install Dependencies:**
```bash
bun add @supabase/supabase-js
```

**New file: `src/lib/supabase.ts`**
```typescript
import { createClient } from '@supabase/supabase-js';

const SUPABASE_URL = import.meta.env.VITE_SUPABASE_URL;
const SUPABASE_ANON_KEY = import.meta.env.VITE_SUPABASE_ANON_KEY;

export const supabase = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
  auth: {
    // Use custom storage backed by Tauri credentials store
    storage: {
      async getItem(key: string) {
        // Get from Tauri store via command
        const session = await commands.authGetSession();
        return session ? JSON.stringify(session) : null;
      },
      async setItem(key: string, value: string) {
        // Save to Tauri store via command
        const session = JSON.parse(value);
        await commands.authSaveSession(session);
      },
      async removeItem(key: string) {
        await commands.authLogout();
      },
    },
    autoRefreshToken: true,
    persistSession: true,
    detectSessionInUrl: false, // We handle deep links manually
  },
});
```

**New file: `src/hooks/useAuth.ts`**
```typescript
import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-shell';
import { supabase } from '@/lib/supabase';
import { commands } from '@/bindings';

interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  user: { id: string; email?: string } | null;
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
      const user = await commands.authGetUser();
      setState({
        isAuthenticated: user.is_authenticated,
        isLoading: false,
        user: user.is_authenticated ? { id: user.id, email: user.email ?? undefined } : null,
        error: null,
      });
    };
    checkAuth();
  }, []);

  // Listen for auth callbacks from local server
  useEffect(() => {
    const unlisten = listen<string>('auth://callback', async (event) => {
      const code = event.payload; // Server sends just the code directly

      if (code && isAuthenticating.current) {
        try {
          // Exchange code for session using PKCE
          const { data, error } = await supabase.auth.exchangeCodeForSession(code);
          if (error) throw error;

          if (data.session) {
            // Save session to Tauri credentials store
            await commands.authSaveSession({
              access_token: data.session.access_token,
              refresh_token: data.session.refresh_token,
              expires_at: data.session.expires_at ?? 0,
              user_id: data.session.user.id,
              email: data.session.user.email ?? null,
            });

            setState({
              isAuthenticated: true,
              isLoading: false,
              user: {
                id: data.session.user.id,
                email: data.session.user.email ?? undefined
              },
              error: null,
            });
          }
        } catch (e) {
          setState(prev => ({ ...prev, error: String(e), isLoading: false }));
        } finally {
          isAuthenticating.current = false;
          // Stop the auth server
          await commands.authStopServer();
        }
      }
    });

    // Also listen for auth errors
    const unlistenError = listen<string>('auth://error', async (event) => {
      setState(prev => ({ ...prev, error: event.payload, isLoading: false }));
      isAuthenticating.current = false;
      await commands.authStopServer();
    });

    return () => {
      unlisten.then(fn => fn());
      unlistenError.then(fn => fn());
    };
  }, []);

  // Sign in with OAuth provider
  const signIn = useCallback(async (provider: 'google' | 'discord' | 'github') => {
    setState(prev => ({ ...prev, isLoading: true, error: null }));
    isAuthenticating.current = true;

    try {
      // 1. Start the local callback server and get the callback URL
      const callbackUrl = await commands.authStartServer();

      // 2. Generate OAuth URL with the dynamic callback
      const { data, error } = await supabase.auth.signInWithOAuth({
        provider,
        options: {
          redirectTo: callbackUrl,
          skipBrowserRedirect: true, // We'll open manually
        },
      });

      if (error) throw error;

      // 3. Open OAuth URL in system browser
      if (data.url) {
        await open(data.url);
      }
    } catch (e) {
      setState(prev => ({ ...prev, error: String(e), isLoading: false }));
      isAuthenticating.current = false;
      // Clean up server on error
      await commands.authStopServer();
    }
  }, []);

  // Cancel ongoing auth flow
  const cancelAuth = useCallback(async () => {
    isAuthenticating.current = false;
    await commands.authStopServer();
    setState(prev => ({ ...prev, isLoading: false }));
  }, []);

  // Sign out
  const signOut = useCallback(async () => {
    await supabase.auth.signOut();
    await commands.authLogout();
    setState({
      isAuthenticated: false,
      isLoading: false,
      user: null,
      error: null,
    });
  }, []);

  return {
    ...state,
    signIn,
    signOut,
    cancelAuth,
  };
}
```

### Phase 4: Auth UI Components

**New file: `src/components/auth/AuthButton.tsx`**
```typescript
import { useAuth } from '@/hooks/useAuth';
import { useTranslation } from 'react-i18next';

export function AuthButton() {
  const { t } = useTranslation();
  const { isAuthenticated, isLoading, user, signIn, signOut } = useAuth();

  if (isLoading) {
    return <button disabled className="...">{t('auth.loading')}</button>;
  }

  if (isAuthenticated && user) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-sm">{user.email}</span>
        <button onClick={signOut} className="...">
          {t('auth.signOut')}
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <button onClick={() => signIn('google')} className="...">
        {t('auth.signInWithGoogle')}
      </button>
      <button onClick={() => signIn('discord')} className="...">
        {t('auth.signInWithDiscord')}
      </button>
      <button onClick={() => signIn('github')} className="...">
        {t('auth.signInWithGithub')}
      </button>
    </div>
  );
}
```

**Modify: `src/i18n/locales/en/translation.json`**
```json
{
  "auth": {
    "loading": "Loading...",
    "signIn": "Sign In",
    "signOut": "Sign Out",
    "signInWithGoogle": "Sign in with Google",
    "signInWithDiscord": "Sign in with Discord",
    "signInWithGithub": "Sign in with GitHub",
    "authenticated": "Signed in as {{email}}"
  }
}
```

### Phase 5: Environment Configuration

**New file: `.env.example`**
```env
VITE_SUPABASE_URL=https://your-project.supabase.co
VITE_SUPABASE_ANON_KEY=your-anon-key-here
```

**Add to `.gitignore`:**
```
.env
.env.local
```

---

## Supabase Dashboard Configuration

1. **Create Project** at [supabase.com](https://supabase.com)

2. **Enable OAuth Providers**:
   - Authentication → Providers → Enable desired providers
   - Configure each provider with their OAuth credentials

3. **Configure Redirect URLs**:
   - Authentication → URL Configuration
   - Add `http://127.0.0.1:4321/auth/callback` to allowed redirect URLs
   - **Note**: Port 4321 is the fixed port used by the app

4. **Get API Keys**:
   - Settings → API
   - Copy `URL` and `anon/public` key to `.env`

---

## Security Considerations

1. **PKCE Flow**: Required for desktop apps (no client secret exposed)
2. **Localhost Only**: Server binds to `127.0.0.1` only (not `0.0.0.0`)
3. **Ephemeral Server**: Server only runs during auth flow, shuts down after
4. **Fixed Port**: Port 4321 is whitelisted in Supabase dashboard
5. **Token Storage**: Access/refresh tokens stored in `credentials.json` via `tauri-plugin-store`
6. **No Secrets in Code**: Anon key is safe to expose (RLS protects data)
7. **Auto Refresh**: Supabase client handles token refresh automatically
8. **Secure Storage**: Same credential store pattern as Discord bot token

---

## Critical Files Summary

| File | Changes |
|------|---------|
| `src-tauri/Cargo.toml` | Add `urlencoding = "2"` |
| `src-tauri/src/auth_server.rs` | NEW - Local HTTP callback server |
| `src-tauri/src/commands/auth.rs` | NEW - Auth commands + AuthManager |
| `src-tauri/src/commands/mod.rs` | Export auth module |
| `src-tauri/src/lib.rs` | Register auth_server module + auth commands + AuthManager state |
| `src/lib/supabase.ts` | NEW - Supabase client config |
| `src/hooks/useAuth.ts` | NEW - Auth hook with server lifecycle |
| `src/components/auth/AuthButton.tsx` | NEW - Auth UI component |
| `src/i18n/locales/en/translation.json` | Add auth translations |
| `.env.example` | NEW - Environment template |

---

## Verification

1. **Build & Run**: `bun run tauri dev`
2. **Click Sign In**: Server starts on port 4321, browser opens to OAuth provider
3. **Check Logs**: Should see "Auth server started on port 4321"
4. **Authenticate**: Complete OAuth flow in browser
5. **Callback**: Browser redirects to `http://127.0.0.1:4321/auth/callback`
6. **Success Page**: Browser shows "Authentication Successful" page
7. **Token Exchange**: App receives code, exchanges for session
8. **Server Shutdown**: Auth server stops automatically
9. **Persist**: Close & reopen app, should still be authenticated
10. **Sign Out**: Clear session, return to sign-in state

---

## Future Enhancements

- **Profile Sync**: Sync user settings to Supabase
- **Cloud Memory**: Store conversation memory in Supabase (per-user)
- **Premium Features**: Gate features behind auth status
- **Multi-Device**: Sync state across devices via Supabase Realtime
