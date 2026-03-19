use clap::Parser;

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

    /// Force auto-submit after transcription (sent to running instance)
    #[arg(long, value_parser = ["enter", "ctrl+enter", "cmd+enter"], default_missing_value = "enter", num_args = 0..=1)]
    pub auto_submit: Option<String>,

    /// Enable debug mode with verbose logging
    #[arg(long)]
    pub debug: bool,
}
