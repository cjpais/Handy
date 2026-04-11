[![Build status](https://img.shields.io/github/actions/workflow/status/enigo-rs/enigo/build.yml?branch=main)](https://github.com/enigo-rs/enigo/actions/workflows/build.yml)
[![Docs](https://docs.rs/enigo/badge.svg)](https://docs.rs/enigo)
[![Dependency status](https://deps.rs/repo/github/enigo-rs/enigo/status.svg)](https://deps.rs/repo/github/enigo-rs/enigo)

![Rust version](https://img.shields.io/badge/rust--version-1.85+-brightgreen.svg)
[![Crates.io](https://img.shields.io/crates/v/enigo.svg)](https://crates.io/crates/enigo)

# enigo

Cross platform input simulation in Rust!

- [x] Serialize/Deserialize
- [x] Linux (X11) mouse
- [x] Linux (X11) text
- [x] Linux (Wayland) mouse (Experimental)
- [x] Linux (Wayland) text (Experimental)
- [x] Linux (libei) mouse (Experimental)
- [x] Linux (libei) text (Experimental)
- [x] MacOS mouse
- [x] MacOS text
- [x] Windows mouse
- [x] Windows text

Enigo also works on *BSDs if they use X11 or Wayland. I don't have a machine to test it and there are no Github Action runners for it, so the BSD support is not explicitly listed.

```Rust
let mut enigo = Enigo::new(&Settings::default()).unwrap();

enigo.move_mouse(500, 200, Abs).unwrap();
enigo.button(Button::Left, Click).unwrap();
enigo.text("Hello World! here is a lot of text  ❤️").unwrap();
```

For more, look at the ([examples](examples)).

## Features

By default, enigo currently works on Windows, macOS and Linux (X11). If you want to be able to serialize and deserialize commands for enigo ([example](examples/serde.rs)), you need to activate the `serde` feature.

There are multiple ways how to simulate input on Linux and not all systems support everything. Enigo can also use wayland protocols and libei to simulate input but there are currently some bugs with it. That is why they are hidden behind feature flags.

## Runtime dependencies

Linux users may have to install `libxdo-dev` if they are using the `xdo` feature. For example, on Debian-based distros:

```Bash
apt install libxdo-dev
```

On Arch:

```Bash
pacman -S xdotool
```

On Fedora:

```Bash
dnf install libX11-devel libxdo-devel
```

On Gentoo:

```Bash
emerge -a xdotool
```

## Permissions

Some platforms have security measures in place to prevent programs from entering keys or controlling the mouse. Have a look at the [permissions](Permissions.md) documentation to see what you need to do to allow it.

## Migrating from a previous version

Please have a look at our [changelog](CHANGES.md) to find out what you have to do, if you used a previous version.

## Debugging

If you encounter an issue and want to debug it, turn on log messages as described [here](DEBUGGING.md).


## Testing this crate

*Warning*: The tests will move the mouse, enter text, press keys and open some applications. Read the test cases before you run them so you know what to expect. It's best to close everything so that the tests don't mess with your system. Some of them run for a long time because they are intended to be run in the CI. Make sure to run the tests sequentially, otherwise they will fail because other mouse movements or entered keys are detected. You can do so by running

```Bash
cargo test --all-features -- --test-threads=1
```