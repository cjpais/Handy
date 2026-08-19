use crate::settings::PasteMethod;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone, Default)]
#[command(name = "handy", about = "Handy - Speech to Text")]
pub struct CliArgs {
    /// Start with the main window hidden
    #[arg(long)]
    pub start_hidden: bool,

    /// Disable the system tray icon
    #[arg(long)]
    pub no_tray: bool,

    /// Toggle transcription on/off (sent to running instance)
    #[arg(long)]
    pub toggle_transcription: bool,

    /// Toggle transcription with post-processing on/off (sent to running instance)
    #[arg(long)]
    pub toggle_post_process: bool,

    /// Cancel the current operation (sent to running instance)
    #[arg(long)]
    pub cancel: bool,

    /// Enable debug mode with verbose logging
    #[arg(long)]
    pub debug: bool,

    /// Control a running Handy instance from scripts and external devices
    #[command(subcommand)]
    pub command: Option<RemoteCommand>,

    /// Transcribe this WAV (16 kHz mono) headlessly and exit. Runs the same
    /// batch transcription path as the app — no mic, no VAD, no download
    /// (the model must already be installed).
    #[arg(short = 'f', long, value_name = "WAV")]
    pub transcribe_file: Option<PathBuf>,

    /// Model id to load for --transcribe-file (default: the selected model).
    #[arg(long)]
    pub model: Option<String>,

    /// Hard-select the compute device for --transcribe-file by its registry
    /// index (see --list-devices). Omit to use the persisted accelerator
    /// setting. transcribe-cpp (whisper-family) models only.
    #[arg(long, value_name = "N")]
    pub device_index: Option<usize>,

    /// List the transcribe-cpp compute devices (with indices) and exit.
    #[arg(long)]
    pub list_devices: bool,

    /// List the available models (with ids) and exit. Pass an id to --model.
    /// Honors --json for machine-readable output.
    #[arg(long)]
    pub list_models: bool,

    /// Repeat the transcription N times (best_ms reports the fastest run).
    #[arg(long, value_name = "N")]
    pub repeat: Option<usize>,

    /// Emit --transcribe-file results as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommand {
    /// Control the recording lifecycle
    Recording {
        #[command(subcommand)]
        action: RecordingCommand,
    },

    /// Deliver a transcript from Handy's history
    Transcript {
        #[command(subcommand)]
        action: TranscriptCommand,
    },
}

#[derive(Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingCommand {
    /// Start recording if Handy is idle
    Start {
        /// Apply the configured post-processing when the recording stops
        #[arg(long)]
        post_process: bool,
    },

    /// Stop the active recording, preserving the mode it was started with
    Stop,

    /// Start recording when idle, or stop the active recording
    Toggle {
        /// Apply the configured post-processing when starting a recording
        #[arg(long)]
        post_process: bool,
    },

    /// Cancel the current recording or transcription operation
    Cancel,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum TranscriptCommand {
    /// Copy the most recent completed transcript to the clipboard
    Clipboard,

    /// Paste the most recent completed transcript into the active application
    Paste {
        /// Override Handy's configured paste method for this invocation
        #[arg(long, value_enum)]
        method: Option<CliPasteMethod>,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliPasteMethod {
    #[value(name = "ctrl_v", alias = "ctrl-v")]
    CtrlV,
    #[value(name = "ctrl_shift_v", alias = "ctrl-shift-v")]
    CtrlShiftV,
    #[value(name = "shift_insert", alias = "shift-insert")]
    ShiftInsert,
    Direct,
    None,
    #[value(name = "external_script", alias = "external-script")]
    ExternalScript,
}

impl From<CliPasteMethod> for PasteMethod {
    fn from(method: CliPasteMethod) -> Self {
        match method {
            CliPasteMethod::CtrlV => Self::CtrlV,
            CliPasteMethod::CtrlShiftV => Self::CtrlShiftV,
            CliPasteMethod::ShiftInsert => Self::ShiftInsert,
            CliPasteMethod::Direct => Self::Direct,
            CliPasteMethod::None => Self::None,
            CliPasteMethod::ExternalScript => Self::ExternalScript,
        }
    }
}

impl CliArgs {
    /// Resolve either the structured subcommands or their legacy flag aliases.
    pub fn remote_command(&self) -> Option<RemoteCommand> {
        if let Some(command) = &self.command {
            return Some(command.clone());
        }

        if self.toggle_transcription {
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Toggle {
                    post_process: false,
                },
            })
        } else if self.toggle_post_process {
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Toggle { post_process: true },
            })
        } else if self.cancel {
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Cancel,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recording_toggle_with_post_processing() {
        let args = CliArgs::try_parse_from(["handy", "recording", "toggle", "--post-process"])
            .expect("structured recording command should parse");

        assert_eq!(
            args.remote_command(),
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Toggle { post_process: true }
            })
        );
    }

    #[test]
    fn parses_idempotent_recording_commands() {
        let start = CliArgs::try_parse_from(["handy", "recording", "start"])
            .expect("recording start should parse");
        let stop = CliArgs::try_parse_from(["handy", "recording", "stop"])
            .expect("recording stop should parse");
        let cancel = CliArgs::try_parse_from(["handy", "recording", "cancel"])
            .expect("recording cancel should parse");

        assert_eq!(
            start.remote_command(),
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Start {
                    post_process: false
                }
            })
        );
        assert_eq!(
            stop.remote_command(),
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Stop
            })
        );
        assert_eq!(
            cancel.remote_command(),
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Cancel
            })
        );
    }

    #[test]
    fn parses_transcript_with_terminal_paste() {
        let args =
            CliArgs::try_parse_from(["handy", "transcript", "paste", "--method", "ctrl_shift_v"])
                .expect("structured transcript command should parse");

        assert_eq!(
            args.remote_command(),
            Some(RemoteCommand::Transcript {
                action: TranscriptCommand::Paste {
                    method: Some(CliPasteMethod::CtrlShiftV)
                }
            })
        );
    }

    #[test]
    fn paste_method_accepts_hyphenated_alias() {
        let args =
            CliArgs::try_parse_from(["handy", "transcript", "paste", "--method", "ctrl-shift-v"])
                .expect("hyphenated paste method alias should parse");

        assert_eq!(
            args.remote_command(),
            Some(RemoteCommand::Transcript {
                action: TranscriptCommand::Paste {
                    method: Some(CliPasteMethod::CtrlShiftV)
                }
            })
        );
    }

    #[test]
    fn legacy_toggle_flag_maps_to_structured_command() {
        let args = CliArgs::try_parse_from(["handy", "--toggle-transcription"])
            .expect("legacy toggle flag should still parse");

        assert_eq!(
            args.remote_command(),
            Some(RemoteCommand::Recording {
                action: RecordingCommand::Toggle {
                    post_process: false
                }
            })
        );
    }

    #[test]
    fn transcript_paste_defaults_to_configured_method() {
        let args = CliArgs::try_parse_from(["handy", "transcript", "paste"])
            .expect("transcript paste should parse");

        assert_eq!(
            args.remote_command(),
            Some(RemoteCommand::Transcript {
                action: TranscriptCommand::Paste { method: None }
            })
        );
    }
}
