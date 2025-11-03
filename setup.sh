#!/bin/bash

echo "🚀 Installation et lancement automatique de Handy pour macOS"
echo "-----------------------------------------------------------"

# --- Vérification de Homebrew ---
if ! command -v brew &>/dev/null; then
  echo "⚠️  Homebrew n'est pas installé. Installation..."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  echo "✅ Homebrew installé."
else
  echo "✅ Homebrew est déjà installé ($(brew -v | head -n 1))"
fi

# --- Vérification de Rust ---
echo "🦀 Vérification de Rust..."
if ! command -v rustc &>/dev/null; then
  echo "⚠️  Rust n'est pas installé."
  if [[ "$1" == "--silent" ]]; then
    export RUSTUP_INIT_SKIP_PATH_CHECK=yes
    curl -sSf https://sh.rustup.rs | sh -s -- -y --quiet
  else
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  fi
  source "$HOME/.cargo/env"
  echo "✅ Rust installé ($(rustc --version))"
else
  echo "✅ Rust est déjà installé ($(rustc --version))"
fi

# --- Vérification de Node.js ---
echo "🧰 Vérification de Node.js..."
if ! command -v node &>/dev/null; then
  echo "⚠️  Node.js n'est pas installé. Installation..."
  brew install node
else
  echo "✅ Node.js est déjà installé ($(node -v))"
fi

# --- Vérification de Bun ---
echo "🍞 Vérification de Bun..."
if ! command -v bun &>/dev/null; then
  echo "⚠️  Bun n'est pas installé. Installation..."
  curl -fsSL https://bun.sh/install | bash
  source "$HOME/.bashrc" 2>/dev/null || source "$HOME/.zshrc" 2>/dev/null
  echo "✅ Bun installé ($(bun --version))"
else
  echo "✅ Bun est déjà installé ($(bun --version))"
fi

# --- Installation des dépendances ---
echo "📦 Installation des dépendances frontend et backend..."
bun install
bun add i18next react-i18next --silent

# --- Compilation du projet ---
echo "🏗️ Compilation de Handy..."
bun run tauri build || { echo "❌ Échec du build Tauri"; exit 1; }

# --- Lancement automatique ---
APP_PATH="src-tauri/target/release/bundle/macos/Handy.app"
if [ -d "$APP_PATH" ]; then
  echo "🎯 Lancement de Handy.app..."
  open "$APP_PATH"
  echo "✅ Handy est en cours d’exécution !"
else
  echo "❌ Handy.app introuvable. Vérifie le build ou exécute : bun run tauri dev"
fi

echo "🎉 Installation et lancement terminés avec succès !"
