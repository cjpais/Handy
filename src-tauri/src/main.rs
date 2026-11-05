// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use handy_app_lib::CliArgs;

fn main() {
    handy_app_lib::webkit_renderer::apply();

    #[cfg(target_os = "windows")]
    {
        // Avoid overlay/capture layer crashes (#2049). Set before backend
        // initialization, preserving user overrides.
        if std::env::var_os("VK_LOADER_LAYERS_DISABLE").is_none()
            && !handy_app_lib::env_flag_enabled("HANDY_KEEP_VULKAN_IMPLICIT_LAYERS")
        {
            std::env::set_var("VK_LOADER_LAYERS_DISABLE", "~implicit~");
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        // Reloading a model after an idle unload takes ~70s on NVIDIA when
        // ggml-vulkan uses host-visible VRAM (#1841). Disable it by default,
        // preserving user overrides; HANDY_KEEP_VULKAN_HOST_VISIBLE_VIDMEM=1
        // opts back in.
        if std::env::var_os("GGML_VK_DISABLE_HOST_VISIBLE_VIDMEM").is_none()
            && !handy_app_lib::env_flag_enabled("HANDY_KEEP_VULKAN_HOST_VISIBLE_VIDMEM")
        {
            std::env::set_var("GGML_VK_DISABLE_HOST_VISIBLE_VIDMEM", "1");
        }
    }

    // Transcription worker: runs transcribe.cpp in isolation and never touches
    // Tauri, the CLI, or single-instance handling. Checked after the env setup
    // above so the worker inherits it (e.g. the Vulkan opt-outs).
    if handy_app_lib::engine_supervisor::is_worker_invocation() {
        std::process::exit(handy_app_lib::engine_supervisor::run_worker());
    }

    let cli_args = CliArgs::parse();
    handy_app_lib::run(cli_args)
}
