#!/bin/bash
set -e

echo "🚀 Installation de Handy pour macOS..."
echo "---------------------------------------"

# Vérification de Homebrew
if ! command -v brew &>/dev/null; then
  echo "📦 Installation de Homebrew..."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
fi

# Dépendances système
echo "🧰 Installation des dépendances nécessaires..."
brew install node rust cargo tauri-cli pkg-config libxkbcommon

# Vérification des architectures M1/M2
if [[ $(uname -m) == 'arm64' ]]; then
  echo "🍎 Architecture ARM détectée (M1/M2) : configuration spécifique..."
  export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="clang"
fi

# Installation des dépendances Node
echo "📦 Installation des dépendances NPM..."
npm install

# Build Tauri
echo "⚙️ Construction de l’application Tauri..."
npm run tauri build || npm run tauri dev

echo "✅ Installation terminée avec succès !"
echo "Lance l’application avec : npm run tauri dev"
