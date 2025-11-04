# Handy 🇫🇷

[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?style=for-the-badge\&logo=discord\&logoColor=white)](https://discord.com/invite/WVBeWsNXK4)

**Une application de reconnaissance vocale gratuite, open source et extensible, fonctionnant entièrement hors ligne.**

Handy est une application de bureau multiplateforme construite avec **Tauri (Rust + React/TypeScript)**.
Elle fournit une transcription vocale simple, respectueuse de la vie privée. Appuyez sur un raccourci clavier, parlez, et vos mots apparaissent dans n’importe quel champ de texte — sans jamais envoyer votre voix sur le cloud.

---

## 🎯 Pourquoi Handy ?

Handy a été créé pour combler le manque d’un véritable outil open source et extensible de reconnaissance vocale.
Comme indiqué sur [handy.computer](https://handy.computer) :

* **Gratuit** : Les outils d’accessibilité doivent être à la portée de tous, pas derrière un paywall.
* **Open Source** : Ensemble, nous pouvons aller plus loin. Étendez Handy selon vos besoins et contribuez à quelque chose de plus grand.
* **Privé** : Votre voix reste sur votre ordinateur. Obtenez des transcriptions sans envoyer d’audio dans le cloud.
* **Simple** : Un outil, une mission. Transcrivez ce que vous dites et insérez-le directement dans un champ de texte.

Handy ne cherche pas à être la meilleure application de reconnaissance vocale — mais la plus **modifiable** et **forkable**.

---

## ⚙️ Comment ça fonctionne

1. **Appuyez** sur un raccourci clavier configurable pour démarrer/arrêter l’enregistrement (ou utilisez le mode push-to-talk).
2. **Parlez** tant que le raccourci est maintenu.
3. **Relâchez** et Handy traite votre voix grâce à Whisper.
4. **Recevez** le texte transcrit automatiquement dans l’application active.

Tout le processus est **entièrement local** :

* Les silences sont filtrés grâce à **VAD (Voice Activity Detection)** avec **Silero**.
* La transcription utilise le modèle de votre choix :

  * **Whisper** (Small / Medium / Turbo / Large) avec accélération GPU quand disponible.
  * **Parakeet V3**, un modèle optimisé CPU avec d’excellentes performances et détection automatique de la langue.

Compatible avec **Windows**, **macOS** et **Linux**.

---

## 🚀 Démarrage rapide

### Installation

1. Téléchargez la dernière version depuis la [page des releases](https://github.com/cjpais/Handy/releases) ou le [site officiel](https://handy.computer).
2. Installez l’application en suivant les instructions spécifiques à votre système d’exploitation.
3. Lancez Handy et accordez les autorisations nécessaires (microphone, accessibilité).
4. Configurez vos raccourcis clavier préférés dans les **Paramètres**.
5. Commencez à transcrire !

### 🧰 Installation automatique sur macOS

Clonez le dépôt et exécutez le script d’installation :

```bash
git clone https://github.com/aurlro/Handy.git
cd Handy
./setup.sh


---

## 🧩 Environnement de développement

Pour les instructions de compilation détaillées, y compris les dépendances spécifiques à chaque système, consultez le fichier [BUILD.md](BUILD.md).

---

## 🏗️ Architecture

Handy est conçu comme une application **Tauri** combinant :

* **Frontend** : React + TypeScript avec Tailwind CSS pour l’interface de configuration.
* **Backend** : Rust pour l’intégration système, le traitement audio et l’inférence des modèles ML.

### Bibliothèques principales

* `whisper-rs` — Reconnaissance vocale locale avec les modèles Whisper.
* `transcription-rs` — Reconnaissance vocale optimisée CPU avec les modèles Parakeet.
* `cpal` — Entrée/sortie audio multiplateforme.
* `vad-rs` — Détection d’activité vocale.
* `rdev` — Gestion des raccourcis clavier globaux et des événements système.
* `rubato` — Rééchantillonnage audio.

---

## ⚙️ Paramètres principaux

Les paramètres de Handy sont organisés en sections **Général**, **Avancé**, **Historique** et **Debug**. Toutes sont désormais entièrement localisées en anglais et en français :

* **Historique** :
  * consulter et réécouter chaque transcription,
  * copier le texte en un clic,
  * marquer des entrées comme favoris ou les supprimer.
* **Debug** :
  * choisir la méthode de collage (simulation du raccourci ou saisie directe),
  * définir la gestion du presse-papiers après insertion,
  * sélectionner un thème sonore et le pré-écouter,
  * ajuster le seuil de correction des mots et la limite d’entrées stockées,
  * activer le micro en permanence ou couper le son système pendant l’enregistrement.

## 🧠 Mode développeur / Debug

Handy inclut un **mode debug avancé** pour le développement et le diagnostic.
Pour y accéder, utilisez le raccourci :

* **macOS** : `Cmd + Maj + D`
* **Windows / Linux** : `Ctrl + Maj + D`

---

## 🌍 Internationalisation

Depuis la version **1.3.0**, Handy est disponible en plusieurs langues :

* 🇬🇧 **Anglais (par défaut)**
* 🇫🇷 **Français**
* 🇪🇸 **Espagnol**
* 📘 **Documentation** : [English](BUILD.md) | [Français](BUILD.fr.md)
* 🔤 **Guide d'internationalisation** : [LOCALIZATION.md](LOCALIZATION.md)

La langue peut être sélectionnée depuis le menu **Paramètres → Langue**.
Toutes les sections de paramètres (Général, Avancé, Historique, Debug) sont synchronisées dans les trois langues et le choix reste enregistré pour les prochaines sessions.

### Ajouter une nouvelle langue

Si vous souhaitez contribuer en ajoutant une nouvelle langue à Handy, consultez notre [Guide d'internationalisation](LOCALIZATION.md) qui explique en détail le processus d'ajout d'une nouvelle langue, les bonnes pratiques et les ressources utiles.

---

## 🤝 Contribution

Les contributions sont les bienvenues !
Vous pouvez proposer de nouvelles traductions, corriger des bugs ou ajouter des fonctionnalités :

1. Forkez le projet
2. Créez une branche :

   ```bash
   git checkout -b feature/traduction-fr
   ```
3. Soumettez une Pull Request.

---

## 🪪 Licence

Projet sous licence **MIT**.
Voir le fichier [LICENSE](LICENSE) pour plus de détails.

---

## 📚 Ressources utiles

* Site officiel : [handy.computer](https://handy.computer)
* Discord communautaire : [Rejoindre](https://discord.com/invite/WVBeWsNXK4)
* Documentation technique : [Wiki du projet](https://github.com/cjpais/Handy/wiki)


### 🧩 Nouveautés de la version 1.4.0

- **Détection automatique de la langue du système** : Handy sélectionne automatiquement la langue de ton système via `navigator.language` lors du premier lancement.  
- **Sélecteur de langue initial** : un menu de sélection apparaît dès le premier démarrage pour choisir la langue de l’interface.  
- **Traduction étendue** : toutes les pages et labels de l’interface ont été traduits.  
- **Support de l'espagnol** : l'application est désormais disponible en espagnol, en plus du français et de l'anglais.
- **Documentation multilingue** : un fichier [`BUILD.fr.md`](BUILD.fr.md) a été ajouté pour les instructions de compilation en français.  
- **Guide d'internationalisation** : un guide complet pour ajouter de nouvelles langues a été créé.


---

## Internationalization

Since version **1.3.0**, Handy is available in multiple languages:

- 🇬🇧 **English (default)**
- 🇫🇷 **French**
- 🇪🇸 **Spanish**
- 📘 **Documentation**: [English](BUILD.md) | [French](BUILD.fr.md)
- 🔤 **Localization Guide**: [LOCALIZATION.md](LOCALIZATION.md)

Language can be selected from the **Settings → Language** menu.

### Adding a New Language

If you'd like to contribute by adding a new language to Handy, check our [Localization Guide](LOCALIZATION.md) which explains in detail the process of adding a new language, best practices, and useful resources.

### What's New in Version 1.4.0

- **Automatic System Language Detection**: Handy automatically selects your system language via `navigator.language` on first launch.  
- **Initial Language Selector**: A selection menu appears on first startup to choose the interface language.  
- **Extended Translation**: All pages and interface labels have been translated.  
- **Spanish Language Support**: The application is now available in Spanish, in addition to French and English.
- **Multilingual Documentation**: A [`BUILD.fr.md`](BUILD.fr.md) file has been added for compilation instructions in French.
- **Localization Guide**: A comprehensive guide for adding new languages has been added.

---

# Handy 🇬🇧

[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?style=for-the-badge&logo=discord&logoColor=white)](https://discord.com/invite/WVBeWsNXK4)

**A free, open source, and extensible speech-to-text application that works completely offline.**

Handy is a cross-platform desktop application built with Tauri (Rust + React/TypeScript) that provides simple, privacy-focused speech transcription. Press a shortcut, speak, and have your words appear in any text field—all without sending your voice to the cloud.

## Why Handy?

Handy was created to fill the gap for a truly open source, extensible speech-to-text tool. As stated on [handy.computer](https://handy.computer):

- **Free**: Accessibility tooling belongs in everyone's hands, not behind a paywall
- **Open Source**: Together we can build further. Extend Handy for yourself and contribute to something bigger
- **Private**: Your voice stays on your computer. Get transcriptions without sending audio to the cloud
- **Simple**: One tool, one job. Transcribe what you say and put it into a text box

Handy isn't trying to be the best speech-to-text app—it's trying to be the most forkable one.

## How It Works

1. **Press** a configurable keyboard shortcut to start/stop recording (or use push-to-talk mode)
2. **Speak** your words while the shortcut is active
3. **Release** and Handy processes your speech using Whisper
4. **Get** your transcribed text pasted directly into whatever app you're using

The process is entirely local:
- Silence is filtered using VAD (Voice Activity Detection) with Silero
- Transcription uses your choice of models:
  - **Whisper models** (Small/Medium/Turbo/Large) with GPU acceleration when available
  - **Parakeet V3** - CPU-optimized model with excellent performance and automatic language detection
- Works on Windows, macOS, and Linux

## Quick Start

### Installation

1. Download the latest release from the [releases page](https://github.com/cjpais/Handy/releases) or the [website](https://handy.computer)
2. Install the application following platform-specific instructions
3. Launch Handy and grant necessary system permissions (microphone, accessibility)
4. Configure your preferred keyboard shortcuts in Settings
5. Start transcribing!

### Development Setup

For detailed build instructions including platform-specific requirements, see [BUILD.md](BUILD.md).

## Architecture

Handy is built as a Tauri application combining:

- **Frontend**: React + TypeScript with Tailwind CSS for the settings UI
- **Backend**: Rust for system integration, audio processing, and ML inference
- **Core Libraries**:
  - `whisper-rs`: Local speech recognition with Whisper models
  - `transcription-rs`: CPU-optimized speech recognition with Parakeet models
  - `cpal`: Cross-platform audio I/O
  - `vad-rs`: Voice Activity Detection
  - `rdev`: Global keyboard shortcuts and system events
  - `rubato`: Audio resampling

## Key Settings

Handy’s settings are split into fully localized **General**, **Advanced**, **History**, and **Debug** sections:

- **History**:
  - review and replay each transcription,
  - copy the transcript instantly,
  - favorite or delete entries as needed.
- **Debug**:
  - pick how Handy pastes text (simulated shortcut vs. direct typing),
  - control clipboard handling after insertion,
  - choose and preview sound themes for start/stop cues,
  - tune the custom-word correction threshold and history retention,
  - keep the microphone always on or mute system audio during recording.

### Debug Mode

Handy includes an advanced debug mode for development and troubleshooting. Access it by pressing:
- **macOS**: `Cmd+Shift+D`
- **Windows/Linux**: `Ctrl+Shift+D`

## Known Issues & Current Limitations

This project is actively being developed and has some [known issues](https://github.com/cjpais/Handy/issues). We believe in transparency about the current state:

### Platform Support
- **macOS (both Intel and Apple Silicon)**
- **x64 Windows**
- **x64 Linux**

### System Requirements/Recommendations

The following are recommendations for running Handy on your own machine. If you don't meet the system requirements, the performance of the application may be degraded. We are working on improving the performance across all kinds of computers and hardware.

**For Whisper Models:**
- **macOS**: M series Mac, Intel Mac
- **Windows**: Intel, AMD, or NVIDIA GPU
- **Linux**: Intel, AMD, or NVIDIA GPU
  * Ubuntu 22.04, 24.04

**For Parakeet V3 Model:**
- **CPU-only operation** - runs on a wide variety of hardware
- **Minimum**: Intel Skylake (6th gen) or equivalent AMD processors
- **Performance**: ~5x real-time speed on mid-range hardware (tested on i5)
- **Automatic language detection** - no manual language selection required

### How to Contribute

1. **Check existing issues** at [github.com/cjpais/Handy/issues](https://github.com/cjpais/Handy/issues)
2. **Fork the repository** and create a feature branch
3. **Test thoroughly** on your target platform
4. **Submit a pull request** with clear description of changes
5. **Join the discussion** - reach out at [contact@handy.computer](mailto:contact@handy.computer)

The goal is to create both a useful tool and a foundation for others to build upon—a well-patterned, simple codebase that serves the community.

## Sponsors

<div align="center">
  We're grateful for the support of our sponsors who help make Handy possible:
  <br><br>
  <a href="https://wordcab.com">
    <img src="sponsor-images/wordcab.png" alt="Wordcab" width="120" height="120">
  </a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://github.com/epicenter-so/epicenter">
    <img src="sponsor-images/epicenter.png" alt="Epicenter" width="120" height="120">
  </a>
</div>

## Related Projects

- **[Handy CLI](https://github.com/cjpais/handy-cli)** - The original Python command-line version
- **[handy.computer](https://handy.computer)** - Project website with demos and documentation

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Acknowledgments

- **Whisper** by OpenAI for the speech recognition model
- **whisper.cpp and ggml** for amazing cross-platform whisper inference/acceleration
- **Silero** for great lightweight VAD
- **Tauri** team for the excellent Rust-based app framework
- **Community contributors** helping make Handy better

---

*"Your search for the right speech-to-text tool can end here—not because Handy is perfect, but because you can make it perfect for you."*
