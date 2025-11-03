#!/bin/bash

echo "🚀 Installation de Handy pour macOS..."
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

  # Mode silencieux (sans prompt utilisateur)
  if [[ "$1" == "--silent" ]]; then
    echo "🤫 Installation silencieuse de Rust..."
    export RUSTUP_INIT_SKIP_PATH_CHECK=yes
    curl -sSf https://sh.rustup.rs | sh -s -- -y --quiet
  else
    echo "📦 Installation de Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  fi

  # Charger les variables d'environnement Cargo
  if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
  fi

  echo "✅ Rust installé avec succès ($(rustc --version))"
else
  echo "✅ Rust est déjà installé ($(rustc --version))"
fin

# --- Vérification de Node.js ---
echo "🧰 Vérification de Node.js..."
if ! command -v node &>/dev/null; then
  echo "⚠️  Node.js n'est pas installé. Installation via Homebrew..."
  brew install node
  echo "✅ Node.js installé ($(node -v))"
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

# --- Fin de l'installation ---
echo "🎉 Installation terminée avec succès !"
echo "👉 Pour lancer Handy en mode développement :"
echo "   bun run tauri dev"
