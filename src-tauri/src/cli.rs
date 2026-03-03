use clap::Parser;

#[derive(Parser, Debug, Clone, Default)]
#[command(name = "handy", about = "Handy - Speech to Text")]
pub struct CliArgs {
    /// Start with the main window hidden
    #[arg(long)]
    pub start_hidden: bool,

    /// Headless mode: transcribe a local audio file and print result to stdout
    #[arg(long, value_name = "PATH")]
    pub transcribe_file: Option<String>,

    /// Output format for --transcribe-file (text|json)
    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    pub format: String,

    /// Optional model id override (defaults to selected model in Handy settings)
    #[arg(long)]
    pub model_id: Option<String>,

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
}
