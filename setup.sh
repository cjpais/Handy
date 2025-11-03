#!/bin/bash

echo "🚀 Installation et lancement de Handy pour macOS..."
echo "---------------------------------------"

# --- Vérification de Homebrew ---
if ! command -v brew &>/dev/null; then
  echo "⚠️  Homebrew n'est pas installé. Installation en cours..."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  echo "✅ Homebrew installé avec succès."
else
  echo "✅ Homebrew est déjà installé ($(brew -v | head -n 1))"
fi

# --- Vérification de Rust ---
echo "🦀 Vérification de Rust..."
if ! command -v rustc &>/dev/null; then
  echo "⚠️  Rust n'est pas installé."
  echo "📦 Installation de Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
  echo "✅ Rust installé avec succès ($(rustc --version))"
else
  echo "✅ Rust est déjà installé ($(rustc --version))"
fi

# --- Vérification de Node.js ---
echo "🧰 Vérification de Node.js..."
if ! command -v node &>/dev/null; then
  echo "⚠️  Node.js n'est pas installé. Installation via Homebrew..."
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

# --- Compilation du projet Tauri ---
echo "🏗️ Compilation de l'application Handy..."
bun run tauri build

# --- Lancement automatique de Handy.app ---
APP_PATH="src-tauri/target/release/bundle/macos/Handy.app"

if [ -d "$APP_PATH" ]; then
  echo "🎯 Lancement de Handy.app..."
  open "$APP_PATH"
  echo "✅ Handy est en cours d’exécution !"
else
  echo "❌ Erreur : l’application Handy.app n’a pas été trouvée à l’emplacement attendu."
  echo "Vérifiez le chemin de sortie ou le type de build (dev/release)."
fi

echo "🎉 Installation et lancement terminés avec succès !"
echo "👉 Pour relancer Handy plus tard :"
echo "   open \"$APP_PATH\""
