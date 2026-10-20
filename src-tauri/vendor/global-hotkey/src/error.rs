// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use thiserror::Error;

use crate::hotkey::HotKey;

/// Errors returned by tray-icon.
#[non_exhaustive]
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    OsError(#[from] std::io::Error),
    #[error("{0}")]
    HotKeyParseError(String),
    #[error("Couldn't recognize \"{0}\" as a valid HotKey Code, if you feel like it should be, please report this to https://github.com/tauri-apps/global-hotkey")]
    UnrecognizedHotKeyCode(String),
    #[error("Unexpected empty token while parsing hotkey: \"{0}\"")]
    EmptyHotKeyToken(String),
    #[error("Unexpected hotkey string format: \"{0}\", a hotkey should have the modifiers first and only contain one main key")]
    UnexpectedHotKeyFormat(String),
    #[error("Unable to register hotkey: {0}")]
    FailedToRegister(String),
    #[error("Failed to unregister hotkey: {0:?}")]
    FailedToUnRegister(HotKey),
    #[error("HotKey already registered: {0:?}")]
    AlreadyRegistered(HotKey),
    #[error("Failed to watch media key event")]
    FailedToWatchMediaKeyEvent,
}

/// Convenient type alias of Result type for tray-icon.
pub type Result<T> = std::result::Result<T, Error>;
