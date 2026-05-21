#[cfg(target_os = "macos")]
use log::debug;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
const MEDIA_REMOTE_PERL: &str = r#"
use strict;
use DynaLoader;
my $cmd = $ARGV[0] // die "usage: $0 <command_id>\n";
my $lib = DynaLoader::dl_load_file("/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote", 0)
    or die "dlopen: " . DynaLoader::dl_error();
my $sym = DynaLoader::dl_find_symbol($lib, "MRMediaRemoteSendCommand")
    or die "dlsym: " . DynaLoader::dl_error();
my $fn = DynaLoader::dl_install_xsub("_mr", $sym, __FILE__);
$fn->(int($cmd), 0);
"#;

#[cfg(target_os = "macos")]
fn send_command(command: u32) -> bool {
    Command::new("/usr/bin/perl")
        .args(["-e", MEDIA_REMOTE_PERL, "--", &command.to_string()])
        .output()
        .map(|o| {
            if !o.status.success() {
                let err = String::from_utf8_lossy(&o.stderr);
                debug!("MediaRemote perl adapter failed: {}", err.trim());
            }
            o.status.success()
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn pause() -> bool {
    send_command(1)
}

#[cfg(target_os = "macos")]
pub fn play() -> bool {
    send_command(0)
}

#[cfg(not(target_os = "macos"))]
pub fn pause() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn play() -> bool {
    false
}
