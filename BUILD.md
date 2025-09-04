# Build Instructions

This guide covers how to set up the development environment and build Handy from source across different platforms.

## Prerequisites

### All Platforms
- [Rust](https://rustup.rs/) (latest stable)
- [Bun](https://bun.sh/) package manager
- [Tauri Prerequisites](https://tauri.app/start/prerequisites/)

### Platform-Specific Requirements

#### macOS
- Xcode Command Line Tools
- Install with: `xcode-select --install`

#### Windows  
- Microsoft C++ Build Tools
- Visual Studio 2019/2022 with C++ development tools
- Or Visual Studio Build Tools 2019/2022

#### Linux
- Build essentials
- ALSA development libraries
- Install with:
  ```bash
  # Ubuntu/Debian
  sudo apt update
  sudo apt install build-essential libasound2-dev pkg-config libssl-dev

  # Fedora/RHEL
  sudo dnf groupinstall "Development Tools"
  sudo dnf install alsa-lib-devel pkgconf openssl-devel

  # Arch Linux
  sudo pacman -S base-devel alsa-lib pkgconf openssl
  ```

## Setup Instructions

### 1. Clone the Repository
```bash
git clone git@github.com:cjpais/Handy.git
cd Handy
```

### 2. Install Dependencies
```bash
bun install
```

### 3. Download Required Models
Handy requires a VAD (Voice Activity Detection) model to function