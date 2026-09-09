//! Built-in keyboard input for Linux.
//!
//! Since Wayland, every desktop has grown its own protocol for faking keyboard
//! input. This module speaks all of them directly over the display socket (no
//! libwayland, no libX11) and picks one from the session it finds:
//!
//! - `zwp_virtual_keyboard_manager_v1` on wlroots compositors (Sway, Hyprland,
//!   ...). It uploads its own keymap and presses synthetic keys, so it types any
//!   character regardless of the layout.
//! - `org_kde_kwin_fake_input`, KWin's privileged protocol, on KDE Plasma, where
//!   the first is not offered. It presses the physical keys that produce each
//!   character on the current layout, so it can only type what that layout
//!   produces; anything else is skipped with a warning. KWin only exposes
//!   fake_input to executables that request it in an installed .desktop file,
//!   so one is installed for Handy on first use.
//! - mutter's private remote-desktop D-Bus API, on the desktops built from
//!   mutter that offer neither Wayland protocol: GNOME, and the forks behind
//!   Cinnamon, Budgie and Pantheon. It takes the same evdev key events as
//!   fake_input and translates them through the session keymap the same way, so
//!   it shares that backend's typing loop and its limitation.
//! - ydotoold, the last resort on a Wayland compositor that offers none of the
//!   above. The daemon owns a uinput device and replays whatever we hand it into
//!   the kernel, so it reaches anything that reads a keyboard, but it has to be
//!   running and its socket has to be writable.
//! - XTEST, on a plain X11 session (no WAYLAND_DISPLAY, but DISPLAY set). Keys
//!   the layout already provides are pressed where they sit, with Shift where
//!   the layout wants it. Only what the layout cannot produce at all is parked
//!   on a keycode it leaves empty, and the mapping is restored afterwards.
//!
//! Wayland wins when both are available: on a Wayland session with Xwayland
//! both variables are set, but only the native protocols reach every window.
//! `HANDY_TYPING_PROTOCOL` pins the choice instead (auto, virtual-keyboard,
//! fake-input, remote-desktop, uinput, xtest), and debug logging narrates the
//! whole decision. libxkbcommon compiles the session's keymap, which is how the
//! keycode-based backends learn where the layout puts each character.

use crate::settings::PasteMethod;
use log::{debug, warn};
use std::ffi::CString;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use xkbcommon::xkb;

type Result<T> = std::result::Result<T, String>;

/// Modifier bits: xkb's real-modifier masks.
const SHIFT: u32 = 1;
const CAPS: u32 = 2;
const CTRL: u32 = 4;
const ALT: u32 = 8;
const LOGO: u32 = 64;
const ALTGR: u32 = 128;

/// evdev key codes, for the backends that press physical keys
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFTALT: u32 = 56;
const KEY_CAPSLOCK: u32 = 58;
const KEY_RIGHTALT: u32 = 100;
const KEY_LEFTMETA: u32 = 125;

/// a few keysyms we need by value
const KS_V: u32 = 0x76;
const KS_TAB: u32 = 0xff09;
const KS_RETURN: u32 = 0xff0d;
const KS_ESCAPE: u32 = 0xff1b;
const KS_INSERT: u32 = 0xff63;
const KS_CAPS_LOCK: u32 = 0xffe5;

/// how long each key is held, in ms
const HOLD_MS: u64 = 2;

/// how both D-Bus and X11 spell the byte order we marshal in
const HOST_ORDER: u8 = if cfg!(target_endian = "little") {
    b'l'
} else {
    b'B'
};

/// what to say when the session turns out to speak none of the protocols
const NO_INPUT_PROTOCOL: &str = "no supported input protocol. This needs a compositor offering \
    the virtual-keyboard protocol (wlroots: Sway, Hyprland, ...), KWin (KDE Plasma), or a \
    mutter-style remote-desktop D-Bus API (GNOME, Cinnamon, Budgie). Failing all of those, \
    start ydotoold and try again.";

enum Cmd<'a> {
    /// type the string
    Text(&'a str),
    /// press and release the keysym
    Tap(u32),
    /// press and hold the modifier
    ModPress(u32),
    /// release the modifier
    ModRelease(u32),
}

/* ------------------------------------------------------------- public API */

pub fn type_text(text: &str) -> Result<()> {
    run(&[Cmd::Text(text)])
}

pub fn send_key_combo(paste_method: &PasteMethod) -> Result<()> {
    let cmds: &[Cmd] = match paste_method {
        PasteMethod::CtrlV => &[Cmd::ModPress(CTRL), Cmd::Tap(KS_V), Cmd::ModRelease(CTRL)],
        PasteMethod::ShiftInsert => &[
            Cmd::ModPress(SHIFT),
            Cmd::Tap(KS_INSERT),
            Cmd::ModRelease(SHIFT),
        ],
        PasteMethod::CtrlShiftV => &[
            Cmd::ModPress(CTRL),
            Cmd::ModPress(SHIFT),
            Cmd::Tap(KS_V),
            Cmd::ModRelease(SHIFT),
            Cmd::ModRelease(CTRL),
        ],
        _ => return Err("Unsupported paste method".into()),
    };
    run(cmds)
}

static AUTO_OFF: AtomicBool = AtomicBool::new(false);

/// Runs `action` on behalf of the automatic tool chain, reporting whether it
/// handled the request. A failure drops the built-in input from that chain for
/// the rest of the session, so a desktop it cannot reach does not pay for a
/// doomed attempt on every paste. Picking it explicitly always tries it.
pub fn try_auto(what: &str, action: impl FnOnce() -> Result<()>) -> bool {
    if AUTO_OFF.load(Ordering::Relaxed) {
        return false;
    }
    match action() {
        Ok(()) => true,
        Err(error) => {
            warn!(
                "Built-in typing could not {what}: {error}. Falling back to the installed \
                 typing tools for the rest of this session."
            );
            AUTO_OFF.store(true, Ordering::Relaxed);
            false
        }
    }
}

/* ---------------------------------------------------------------- helpers */

fn nap(ms: u64) {
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

fn yesno(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

fn get_u16(b: &[u8], off: usize) -> u16 {
    b.get(off..off + 2)
        .map(|s| u16::from_ne_bytes([s[0], s[1]]))
        .unwrap_or(0)
}

fn get_u32(b: &[u8], off: usize) -> u32 {
    b.get(off..off + 4)
        .map(|s| u32::from_ne_bytes([s[0], s[1], s[2], s[3]]))
        .unwrap_or(0)
}

fn keysym_name(ks: u32) -> String {
    xkb::keysym_get_name(xkb::Keysym::new(ks))
}

fn keysym_name_or_q(ks: u32) -> String {
    let name = keysym_name(ks);
    if name.is_empty() {
        "?".into()
    } else {
        name
    }
}

/// the keysym to type for a character, if xkb knows one
fn char_to_keysym(cp: char) -> Option<u32> {
    let ks = match cp {
        '\n' => KS_RETURN,
        '\t' => KS_TAB,
        '\x1b' => KS_ESCAPE,
        _ => xkb::utf32_to_keysym(cp as u32).raw(),
    };
    (ks != 0 && !keysym_name(ks).is_empty()).then_some(ks)
}

/// each character of `text` with its keysym, minus the carriage returns
fn text_keysyms(text: &str) -> impl Iterator<Item = (char, Option<u32>)> + '_ {
    text.chars()
        .filter(|&cp| cp != '\r')
        .map(|cp| (cp, char_to_keysym(cp)))
}

fn mod_name(m: u32) -> &'static str {
    match m {
        SHIFT => "shift",
        CAPS => "capslock",
        CTRL => "ctrl",
        ALT => "alt",
        LOGO => "logo",
        ALTGR => "altgr",
        _ => "?",
    }
}

/// One line per action. The text itself is never logged, only how much of it
/// there is: people dictate passwords.
fn log_cmd(c: &Cmd) {
    match c {
        Cmd::Text(t) => debug!("typing {} characters", t.chars().count()),
        Cmd::Tap(ks) => debug!("tapping {}", keysym_name_or_q(*ks)),
        Cmd::ModPress(m) => debug!("holding the {} modifier", mod_name(*m)),
        Cmd::ModRelease(m) => debug!("releasing the {} modifier", mod_name(*m)),
    }
}

/// What a backend offers `run_cmds`: keys by keysym, modifiers by mask.
trait Typist {
    /// press and release the key that produces `ks`
    fn stroke(&mut self, ks: u32) -> Result<()>;
    /// press or release a modifier; for CAPS, toggle the lock
    fn modifier(&mut self, m: u32, press: bool) -> Result<()>;
    /// whether Caps Lock is on, which would invert letter case
    fn caps_lock_on(&mut self) -> Result<bool> {
        Ok(false)
    }
    /// wait for the last events to land
    fn done(&mut self) -> Result<()>;
}

fn run_cmds(t: &mut impl Typist, cmds: &[Cmd]) -> Result<()> {
    let caps = t.caps_lock_on()?;
    if caps {
        t.modifier(CAPS, true)?;
        nap(20);
    }
    for c in cmds {
        log_cmd(c);
        match c {
            Cmd::Text(text) => {
                for (cp, ks) in text_keysyms(text) {
                    match ks {
                        Some(ks) => t.stroke(ks)?,
                        None => warn!("U+{:04X} has no keysym; skipped", cp as u32),
                    }
                }
            }
            Cmd::Tap(ks) => t.stroke(*ks)?,
            Cmd::ModPress(m) => t.modifier(*m, true)?,
            Cmd::ModRelease(m) => t.modifier(*m, false)?,
        }
    }
    if caps {
        t.modifier(CAPS, true)?;
    }
    t.done()
}

/* ---------------------------------------------------------------- Wayland */

struct Wayland {
    sock: UnixStream,
    next_id: u32,
    registry_id: u32,
    sync_id: u32,
    sync_done: bool,
    keyboard_id: u32,
    /// (name, version) of the globals we care about, once advertised
    fake: Option<(u32, u32)>,
    seat: Option<(u32, u32)>,
    vkm: Option<(u32, u32)>,
    /// the seat's keymap: its fd and size
    keymap: Option<(OwnedFd, u32)>,
    inbuf: Vec<u8>,
    fdq: Vec<OwnedFd>,
}

fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_ne_bytes());
}

fn put_str(b: &mut Vec<u8>, s: &str) {
    put_u32(b, s.len() as u32 + 1);
    b.extend_from_slice(s.as_bytes());
    b.push(0);
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

/// the string at `off` (a length including its NUL, the bytes, padding), and
/// the offset just past it
fn get_str(b: &[u8], off: usize) -> (String, usize) {
    let len = get_u32(b, off) as usize;
    let start = (off + 4).min(b.len());
    let end = (start + len.saturating_sub(1)).min(b.len());
    let s = String::from_utf8_lossy(&b[start..end]).into_owned();
    (s, off + 4 + ((len + 3) & !3))
}

impl Wayland {
    fn connect() -> Result<Wayland> {
        let disp = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let path = if disp.starts_with('/') {
            PathBuf::from(disp)
        } else {
            let rt = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR is not set")?;
            PathBuf::from(rt).join(disp)
        };
        let sock = UnixStream::connect(&path)
            .map_err(|e| format!("cannot connect to the Wayland display: {e}"))?;
        sock.set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| e.to_string())?;
        debug!("connected to the Wayland display at {}", path.display());
        Ok(Wayland {
            sock,
            next_id: 2,
            registry_id: 0,
            sync_id: 0,
            sync_done: false,
            keyboard_id: 0,
            fake: None,
            seat: None,
            vkm: None,
            keymap: None,
            inbuf: Vec::new(),
            fdq: Vec::new(),
        })
    }

    fn alloc_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id - 1
    }

    fn send(&mut self, obj: u32, opcode: u16, body: &[u8]) -> Result<()> {
        self.send_fd(obj, opcode, body, None)
    }

    /// like send, but optionally pass one file descriptor in the ancillary data
    fn send_fd(&mut self, obj: u32, opcode: u16, body: &[u8], fd: Option<&OwnedFd>) -> Result<()> {
        let size = 8 + body.len();
        if size > u16::MAX as usize {
            return Err("outgoing message too large".into());
        }
        let mut buf = Vec::with_capacity(size);
        put_u32(&mut buf, obj);
        put_u32(&mut buf, ((size as u32) << 16) | opcode as u32);
        buf.extend_from_slice(body);

        let Some(fd) = fd else {
            return self
                .sock
                .write_all(&buf)
                .map_err(|e| format!("write to compositor failed: {e}"));
        };

        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut _,
            iov_len: buf.len(),
        };
        let space = unsafe { libc::CMSG_SPACE(4) } as usize;
        let mut cbuf = vec![0u64; space.div_ceil(8)];
        let mut mh: libc::msghdr = unsafe { std::mem::zeroed() };
        mh.msg_iov = &mut iov;
        mh.msg_iovlen = 1;
        mh.msg_control = cbuf.as_mut_ptr() as *mut _;
        mh.msg_controllen = space as _;
        unsafe {
            let c = libc::CMSG_FIRSTHDR(&mh);
            (*c).cmsg_level = libc::SOL_SOCKET;
            (*c).cmsg_type = libc::SCM_RIGHTS;
            (*c).cmsg_len = libc::CMSG_LEN(4) as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(c) as *mut i32, fd.as_raw_fd());
        }
        loop {
            let n = unsafe { libc::sendmsg(self.sock.as_raw_fd(), &mh, 0) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("sendmsg to compositor failed: {e}"));
            }
            return Ok(()); // the message is tiny; a short send will not happen
        }
    }

    fn recv_some(&mut self) -> Result<()> {
        let mut tmp = [0u8; 4096];
        let mut iov = libc::iovec {
            iov_base: tmp.as_mut_ptr() as *mut _,
            iov_len: tmp.len(),
        };
        let space = unsafe { libc::CMSG_SPACE(4 * 8) } as usize;
        let mut cbuf = vec![0u64; space.div_ceil(8)];
        let mut mh: libc::msghdr = unsafe { std::mem::zeroed() };
        mh.msg_iov = &mut iov;
        mh.msg_iovlen = 1;
        mh.msg_control = cbuf.as_mut_ptr() as *mut _;
        mh.msg_controllen = space as _;
        let n = unsafe { libc::recvmsg(self.sock.as_raw_fd(), &mut mh, 0) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            return match e.kind() {
                ErrorKind::Interrupted => Ok(()),
                ErrorKind::WouldBlock => Err("timed out waiting for the compositor".into()),
                _ => Err(format!("read from compositor failed: {e}")),
            };
        }
        if n == 0 {
            return Err("compositor closed the connection".into());
        }
        unsafe {
            let mut c = libc::CMSG_FIRSTHDR(&mh);
            while !c.is_null() {
                if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                    let cnt = ((*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / 4;
                    let fds = libc::CMSG_DATA(c) as *const i32;
                    for i in 0..cnt {
                        self.fdq
                            .push(OwnedFd::from_raw_fd(std::ptr::read_unaligned(fds.add(i))));
                    }
                }
                c = libc::CMSG_NXTHDR(&mh, c);
            }
        }
        self.inbuf.extend_from_slice(&tmp[..n as usize]);
        Ok(())
    }

    fn dispatch_one(&mut self, obj: u32, op: u16, body: &[u8]) -> Result<()> {
        if obj == 1 {
            // wl_display
            if op == 0 {
                // error(object, code, message)
                let (message, _) = get_str(body, 8);
                return Err(format!("wayland error {}: {message}", get_u32(body, 4)));
            }
            return Ok(()); // delete_id: ignore
        }
        if obj == self.registry_id && op == 0 {
            // global(name, interface, version)
            let name = get_u32(body, 0);
            let (iface, off) = get_str(body, 4);
            let ver = get_u32(body, off);
            match iface.as_str() {
                "org_kde_kwin_fake_input" => self.fake = Some((name, ver)),
                "zwp_virtual_keyboard_manager_v1" => self.vkm = Some((name, ver)),
                "wl_seat" => self.seat = Some((name, ver)),
                _ => {}
            }
            return Ok(());
        }
        if obj == self.sync_id && op == 0 {
            self.sync_done = true;
            return Ok(());
        }
        if obj == self.keyboard_id && op == 0 && !self.fdq.is_empty() {
            // keymap(format, fd, size)
            self.keymap = Some((self.fdq.remove(0), get_u32(body, 4)));
        }
        Ok(())
    }

    fn dispatch_pending(&mut self) -> Result<()> {
        let mut off = 0;
        while self.inbuf.len() - off >= 8 {
            let obj = get_u32(&self.inbuf, off);
            let w1 = get_u32(&self.inbuf, off + 4);
            let (size, op) = ((w1 >> 16) as usize, (w1 & 0xffff) as u16);
            if size < 8 {
                return Err("malformed message".into());
            }
            if self.inbuf.len() - off < size {
                break;
            }
            let body = self.inbuf[off + 8..off + size].to_vec();
            self.dispatch_one(obj, op, &body)?;
            off += size;
        }
        self.inbuf.drain(..off);
        Ok(())
    }

    /// flush, then block until the compositor has processed everything so far
    fn roundtrip(&mut self) -> Result<()> {
        self.sync_id = self.alloc_id();
        self.sync_done = false;
        self.send(1, 0, &self.sync_id.to_ne_bytes())?; // wl_display.sync
        while !self.sync_done {
            self.dispatch_pending()?;
            if self.sync_done {
                break;
            }
            self.recv_some()?;
        }
        Ok(())
    }

    fn get_globals(&mut self) -> Result<()> {
        self.registry_id = self.alloc_id();
        self.send(1, 1, &self.registry_id.to_ne_bytes())?; // wl_display.get_registry
        self.roundtrip()
    }

    /// bind an advertised global, at the newest version we speak
    fn bind(&mut self, (name, ver): (u32, u32), iface: &str, max_version: u32) -> Result<u32> {
        let id = self.alloc_id();
        let mut b = Vec::new();
        put_u32(&mut b, name);
        put_str(&mut b, iface);
        put_u32(&mut b, ver.min(max_version));
        put_u32(&mut b, id);
        self.send(self.registry_id, 0, &b)?; // wl_registry.bind
        Ok(id)
    }
}

/* -------------------------------------------------------------- keymaps */

/// The compositor's own keymap, which is how the keycode backends learn where
/// the current layout puts each character.
struct SessionKeymap(xkb::Keymap);

impl SessionKeymap {
    fn compile(fd: OwnedFd, size: u32) -> Result<SessionKeymap> {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = unsafe {
            xkb::Keymap::new_from_fd(
                &ctx,
                fd,
                size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        }
        .map_err(|e| format!("mmap of the compositor keymap failed: {e}"))?
        .ok_or("could not compile the compositor keymap")?;
        Ok(SessionKeymap(keymap))
    }

    /// Where the active layout (group 0) puts `keysym`: the evdev code and
    /// the shift level, plainest first. Levels past Shift+AltGr need keys we
    /// do not press, so those count as unreachable.
    fn find_key(&self, keysym: u32) -> Option<(u32, u32)> {
        let keycodes = self.0.min_keycode().raw()..=self.0.max_keycode().raw();
        (0..4).find_map(|lv| {
            keycodes
                .clone()
                .find(|&kc| {
                    self.0
                        .key_get_syms_by_level(xkb::Keycode::new(kc), 0, lv)
                        .iter()
                        .any(|s| s.raw() == keysym)
                })
                .map(|kc| (kc - 8, lv))
        })
    }
}

/// Read the compositor's keymap off the seat.
fn fetch_seat_keymap(wl: &mut Wayland) -> Result<SessionKeymap> {
    let seat = wl
        .seat
        .ok_or("the compositor offers no wl_seat; cannot read the keymap")?;
    let seat_id = wl.bind(seat, "wl_seat", 5)?;
    wl.keyboard_id = wl.alloc_id();
    wl.send(seat_id, 1, &wl.keyboard_id.to_ne_bytes())?; // wl_seat.get_keyboard
    wl.roundtrip()?;
    let (fd, size) = wl
        .keymap
        .take()
        .ok_or("did not receive a keymap from the seat")?;
    debug!("read the session keymap off wl_seat ({size} bytes)");
    SessionKeymap::compile(fd, size)
}

/// The keysyms a run will type, each given a slot: a key of its own in a
/// keymap we upload (virtual keyboard), or a keycode we borrow (X11).
#[derive(Default)]
struct Slots(Vec<u32>);

impl Slots {
    /// the 1-based slot of `ks`, adding one if needed
    fn slot_for(&mut self, ks: u32) -> u32 {
        if let Some(i) = self.0.iter().position(|&s| s == ks) {
            return i as u32 + 1;
        }
        self.0.push(ks);
        self.0.len() as u32
    }

    /// first pass: give every keysym we will type a slot
    fn collect(cmds: &[Cmd]) -> Slots {
        let mut slots = Slots::default();
        for c in cmds {
            match c {
                Cmd::Tap(ks) => {
                    slots.slot_for(*ks);
                }
                Cmd::Text(text) => {
                    for ks in text_keysyms(text).filter_map(|(_, ks)| ks) {
                        slots.slot_for(ks);
                    }
                }
                _ => {}
            }
        }
        slots
    }
}

/* ------------------------------------------- backend A: virtual keyboard */

/// the keymap text we upload: one key per slot, NUL-terminated as the
/// protocol wants it
fn keymap_text(slots: &[u32]) -> String {
    let mut s = String::from("xkb_keymap {\n");
    let _ = writeln!(
        s,
        "xkb_keycodes \"(unnamed)\" {{\nminimum = 8;\nmaximum = {};",
        slots.len() + 9
    );
    for i in 0..slots.len() {
        let _ = writeln!(s, "<K{}> = {};", i + 1, i + 9);
    }
    s += "};\n";
    s += "xkb_types \"(unnamed)\" { include \"complete\" };\n";
    s += "xkb_compatibility \"(unnamed)\" { include \"complete\" };\n";
    s += "xkb_symbols \"(unnamed)\" {\n";
    for (i, &ks) in slots.iter().enumerate() {
        let mut name = keysym_name(ks);
        if name.is_empty() {
            name = "NoSymbol".into();
        }
        let _ = writeln!(s, "key <K{}> {{[{name}]}};", i + 1);
    }
    s += "};\n};\n\0";
    s
}

struct VirtualKeyboard<'a> {
    wl: &'a mut Wayland,
    id: u32,
    slots: Slots,
    mods: u32,
}

impl VirtualKeyboard<'_> {
    /// build the keymap and hand it to the compositor
    fn upload_keymap(&mut self) -> Result<()> {
        let text = keymap_text(&self.slots.0);
        let name = CString::new("handy-keymap").map_err(|e| e.to_string())?;
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(format!(
                "memfd_create failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        file.write_all(text.as_bytes())
            .map_err(|e| format!("writing the keymap failed: {e}"))?;
        let mut b = Vec::new();
        put_u32(&mut b, 1); // format XKB_V1
        put_u32(&mut b, text.len() as u32);
        self.wl.send_fd(self.id, 0, &b, Some(&file.into()))?; // virtual_keyboard.keymap
        self.wl.roundtrip()?;
        debug!("uploaded a keymap carrying {} keysyms", self.slots.0.len());
        Ok(())
    }

    fn key(&mut self, slot: u32, state: u32) -> Result<()> {
        let mut b = Vec::new();
        put_u32(&mut b, 0);
        put_u32(&mut b, slot);
        put_u32(&mut b, state);
        self.wl.send(self.id, 1, &b)?; // virtual_keyboard.key
        self.wl.roundtrip()
    }
}

impl Typist for VirtualKeyboard<'_> {
    fn stroke(&mut self, ks: u32) -> Result<()> {
        let slot = self.slots.slot_for(ks);
        self.key(slot, 1)?;
        nap(HOLD_MS);
        self.key(slot, 0)?;
        nap(HOLD_MS);
        Ok(())
    }

    fn modifier(&mut self, m: u32, press: bool) -> Result<()> {
        if press {
            self.mods |= m;
        } else {
            self.mods &= !m;
        }
        let mut b = Vec::new();
        put_u32(&mut b, self.mods & !CAPS); // depressed
        put_u32(&mut b, 0); // latched
        put_u32(&mut b, self.mods & CAPS); // locked
        put_u32(&mut b, 0); // group
        self.wl.send(self.id, 2, &b)?; // virtual_keyboard.modifiers
        self.wl.roundtrip()
    }

    fn done(&mut self) -> Result<()> {
        self.wl.roundtrip()
    }
}

fn run_virtual_keyboard(wl: &mut Wayland, cmds: &[Cmd]) -> Result<()> {
    let vkm = wl.vkm.ok_or("the compositor offers no virtual keyboard")?;
    debug!(
        "using virtual-keyboard (zwp_virtual_keyboard_manager_v1, version {})",
        vkm.1
    );
    let seat = wl.seat.ok_or("the compositor offers no wl_seat")?;
    let seat_id = wl.bind(seat, "wl_seat", 7)?;
    let vkm_id = wl.bind(vkm, "zwp_virtual_keyboard_manager_v1", 1)?;
    let id = wl.alloc_id();
    let mut b = Vec::new();
    put_u32(&mut b, seat_id);
    put_u32(&mut b, id);
    wl.send(vkm_id, 0, &b)?; // manager.create_virtual_keyboard

    let mut vk = VirtualKeyboard {
        wl,
        id,
        slots: Slots::collect(cmds),
        mods: 0,
    };
    vk.upload_keymap()?;
    run_cmds(&mut vk, cmds)
}

/* ------------------------------------------ backends B, C and D: keycodes */
/*
 * KWin's fake_input, mutter's remote-desktop D-Bus API and ydotoold all take
 * evdev keycodes and run them through the session's own keymap, so everything
 * from here to the fake_input section is shared: only how a single key event
 * reaches the session differs, and that is what KeyEmitter abstracts.
 */

trait KeyEmitter {
    fn key(&mut self, code: u32, state: u32) -> Result<()>;
    /// wait for the last events to land
    fn done(&mut self) -> Result<()>;
}

fn mod_phys(m: u32) -> u32 {
    match m {
        SHIFT => KEY_LEFTSHIFT,
        CTRL => KEY_LEFTCTRL,
        ALT => KEY_LEFTALT,
        LOGO => KEY_LEFTMETA,
        ALTGR => KEY_RIGHTALT,
        _ => 0,
    }
}

fn caps_lock_led() -> bool {
    let Ok(leds) = std::fs::read_dir("/sys/class/leds") else {
        return false;
    };
    leds.flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("capslock"))
        .any(|e| {
            std::fs::read(e.path().join("brightness"))
                .map(|b| matches!(b.first(), Some(b'1'..=b'9')))
                .unwrap_or(false)
        })
}

struct Keycodes<'a> {
    emit: &'a mut dyn KeyEmitter,
    keymap: &'a SessionKeymap,
    /// modifiers held down via ModPress
    held_mods: u32,
}

impl Keycodes<'_> {
    fn tap(&mut self, code: u32) -> Result<()> {
        self.emit.key(code, 1)?;
        nap(HOLD_MS);
        self.emit.key(code, 0)
    }
}

impl Typist for Keycodes<'_> {
    /// press a keysym on the real layout, adding Shift/AltGr only if not held
    fn stroke(&mut self, ks: u32) -> Result<()> {
        let Some((code, level)) = self.keymap.find_key(ks) else {
            warn!(
                "'{}' is not on the active keyboard layout; skipped",
                keysym_name_or_q(ks)
            );
            return Ok(());
        };
        let ts = (level == 1 || level == 3) && self.held_mods & SHIFT == 0;
        let ta = (level == 2 || level == 3) && self.held_mods & ALTGR == 0;
        if ts {
            self.emit.key(KEY_LEFTSHIFT, 1)?;
        }
        if ta {
            self.emit.key(KEY_RIGHTALT, 1)?;
        }
        self.tap(code)?;
        if ta {
            self.emit.key(KEY_RIGHTALT, 0)?;
        }
        if ts {
            self.emit.key(KEY_LEFTSHIFT, 0)?;
        }
        Ok(())
    }

    fn modifier(&mut self, m: u32, press: bool) -> Result<()> {
        if m == CAPS {
            self.tap(KEY_CAPSLOCK)?;
        } else {
            self.emit.key(mod_phys(m), press as u32)?;
        }
        if press {
            self.held_mods |= m;
        } else {
            self.held_mods &= !m;
        }
        Ok(())
    }

    fn caps_lock_on(&mut self) -> Result<bool> {
        Ok(caps_lock_led())
    }

    fn done(&mut self) -> Result<()> {
        self.emit.done()
    }
}

/* -------------------------------------- backend B: fake_input, on KWin */

struct FakeInput<'a> {
    wl: &'a mut Wayland,
    id: u32,
}

impl KeyEmitter for FakeInput<'_> {
    fn key(&mut self, code: u32, state: u32) -> Result<()> {
        let mut b = Vec::new();
        put_u32(&mut b, code);
        put_u32(&mut b, state);
        self.wl.send(self.id, 10, &b) // fake_input.keyboard_key
    }

    fn done(&mut self) -> Result<()> {
        self.wl.roundtrip()
    }
}

/// `dirs` with the entries an AppImage's AppRun prepended taken back out: its
/// own `$APPDIR/usr/share` (twice, once with a trailing slash) and a duplicate
/// of `/usr/share`.
fn session_data_dirs(dirs: &str, appdir: &str) -> String {
    let mut dirs: Vec<&str> = dirs
        .split(':')
        .skip_while(|dir| dir.starts_with(appdir))
        .collect();
    if dirs.first() == Some(&"/usr/share") && dirs[1..].contains(&"/usr/share") {
        dirs.remove(0);
    }
    dirs.join(":")
}

/// Install the .desktop file that asks KWin for fake_input on our behalf. KWin
/// matches it to us by executable path, so it names this very binary.
fn install_desktop() -> Result<()> {
    let exe =
        std::env::current_exe().map_err(|e| format!("readlink /proc/self/exe failed: {e}"))?;
    let data_home = match std::env::var("XDG_DATA_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var("HOME").map_err(|_| "HOME is not set")?)
            .join(".local/share"),
    };
    let dir = data_home.join("applications");
    std::fs::create_dir_all(&dir)
        .and_then(|_| {
            std::fs::write(
                dir.join("handy-typing.desktop"),
                format!(
                    "[Desktop Entry]\nType=Application\nName=Handy\nNoDisplay=true\n\
                     Terminal=false\nExec={}\nX-KDE-Wayland-Interfaces=org_kde_kwin_fake_input\n",
                    exe.display()
                ),
            )
        })
        .map_err(|e| format!("cannot write the authorization .desktop file: {e}"))?;

    // KWin reads KService's cache rather than the file, so rebuild it with
    // whichever kbuildsycoca this Plasma has. An AppImage's AppRun points
    // LD_LIBRARY_PATH at its own bundle, which breaks the libraries the tool
    // loads, and prepends its tree to XDG_DATA_DIRS, which KService derives the
    // cache's filename from -- so the rebuild would land in a file KWin never
    // reads.
    for tool in ["kbuildsycoca6", "kbuildsycoca5"] {
        let mut command = Command::new(tool);
        command
            .arg("--noincremental")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("LD_PRELOAD");
        if let (Ok(appdir), Ok(dirs)) = (std::env::var("APPDIR"), std::env::var("XDG_DATA_DIRS")) {
            command.env("XDG_DATA_DIRS", session_data_dirs(&dirs, &appdir));
        }
        if command.status().is_ok() {
            break;
        }
    }
    Ok(())
}

/// Make sure fake_input is offered, self-installing the .desktop file if not.
fn ensure_fake_input(wl: Wayland) -> Result<Wayland> {
    if wl.fake.is_some() {
        return Ok(wl);
    }
    debug!("fake_input is not offered yet; installing the .desktop that asks for it");
    drop(wl);
    install_desktop()?;
    for i in 1..=16 {
        // KWin reloads its service cache async
        nap(500);
        let mut wl = Wayland::connect()?;
        wl.get_globals()?;
        if wl.fake.is_some() {
            debug!("fake_input appeared after {} ms", i * 500);
            return Ok(wl);
        }
    }
    Err(
        "org_kde_kwin_fake_input is still unavailable after self-install. \
         A re-login may be needed for KWin to pick up the change."
            .into(),
    )
}

fn run_fake_input(wl: Wayland, cmds: &[Cmd]) -> Result<()> {
    let mut wl = ensure_fake_input(wl)?;
    let fake = wl.fake.ok_or("the compositor offers no fake_input")?;
    debug!(
        "using fake-input (org_kde_kwin_fake_input, version {})",
        fake.1
    );
    if fake.1 < 4 {
        return Err("this KWin's fake_input is too old (need version 4+)".into());
    }
    let id = wl.bind(fake, "org_kde_kwin_fake_input", 6)?;
    let mut b = Vec::new();
    put_str(&mut b, "Handy");
    put_str(&mut b, "typing transcribed text");
    wl.send(id, 0, &b)?; // fake_input.authenticate

    let keymap = fetch_seat_keymap(&mut wl)?;
    let mut emit = FakeInput { wl: &mut wl, id };
    let mut typist = Keycodes {
        emit: &mut emit,
        keymap: &keymap,
        held_mods: 0,
    };
    run_cmds(&mut typist, cmds)
}

/* --------------------------------------- backend C: remote desktop D-Bus */
/*
 * Mutter offers neither Wayland protocol, but it carries a private D-Bus API
 * for remote-desktop servers: org.gnome.Mutter.RemoteDesktop. A session
 * created there accepts evdev key events, which is exactly what fake_input
 * takes, so this backend borrows the typing loop above -- and inherits its
 * limitation, because mutter also looks each keycode up in the session's own
 * keymap.
 *
 * Several desktops are built on mutter, and they do not all keep the name.
 * Cinnamon's muffin renamed the whole API to org.cinnamon.Muffin.RemoteDesktop
 * while leaving its shape untouched; Budgie's magpie and Pantheon's gala kept
 * mutter's names, so they need no entry of their own.
 *
 * D-Bus is spoken straight over the bus socket, like Wayland and X11 elsewhere
 * in this file: no libdbus, no GIO. Only what these few method calls need is
 * implemented -- small fixed-shape messages, no file descriptors, no signal
 * matching, no properties.
 */

/// header field codes, from the D-Bus specification
const D_PATH: u8 = 1;
const D_IFACE: u8 = 2;
const D_MEMBER: u8 = 3;
const D_ERRNAME: u8 = 4;
const D_REPLYSERIAL: u8 = 5;
const D_DEST: u8 = 6;
const D_SIGNATURE: u8 = 8;
/// message types
const D_CALL: u8 = 1;
const D_RETURN: u8 = 2;

/// The names to try, in order: (service, path). Upstream uses the service
/// name as the manager interface too, and the session interface is that plus
/// ".Session"; the forks follow suit, so two strings describe a flavor.
const RD_FLAVORS: [(&str, &str); 2] = [
    (
        "org.gnome.Mutter.RemoteDesktop",
        "/org/gnome/Mutter/RemoteDesktop",
    ),
    (
        "org.cinnamon.Muffin.RemoteDesktop",
        "/org/cinnamon/Muffin/RemoteDesktop",
    ),
];

/// A message under construction. Everything we send is small and fixed-shape,
/// so one flat buffer is enough.
struct DbusMsg {
    b: Vec<u8>,
    /// where the body starts
    body: usize,
}

impl DbusMsg {
    fn pad(&mut self, a: usize) {
        while !self.b.len().is_multiple_of(a) {
            self.b.push(0);
        }
    }

    fn u32(&mut self, v: u32) {
        self.pad(4);
        self.b.extend_from_slice(&v.to_ne_bytes());
    }

    /// STRING, OBJECT_PATH
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.b.extend_from_slice(s.as_bytes());
        self.b.push(0);
    }

    /// SIGNATURE
    fn sig(&mut self, s: &str) {
        self.b.push(s.len() as u8);
        self.b.extend_from_slice(s.as_bytes());
        self.b.push(0);
    }

    /// one header field: a struct, so 8-aligned, of a field code and a variant
    fn field(&mut self, code: u8, ty: &str, val: &str) {
        self.pad(8);
        self.b.push(code);
        self.sig(ty);
        if ty == "g" {
            self.sig(val);
        } else {
            self.str(val);
        }
    }

    /// Open a method call. Arguments matching `sig` are appended by the
    /// caller, then Bus::send() fills in the lengths and writes it out.
    fn call(dest: &str, path: &str, iface: &str, member: &str, sig: &str) -> DbusMsg {
        let mut m = DbusMsg {
            b: vec![0; 16],
            body: 0,
        };
        m.b[0] = HOST_ORDER;
        m.b[1] = D_CALL;
        m.b[3] = 1; // protocol version; body length and serial are patched in later
        m.field(D_PATH, "o", path);
        m.field(D_IFACE, "s", iface);
        m.field(D_MEMBER, "s", member);
        m.field(D_DEST, "s", dest);
        if !sig.is_empty() {
            m.field(D_SIGNATURE, "g", sig);
        }
        let hlen = (m.b.len() - 16) as u32;
        m.b[12..16].copy_from_slice(&hlen.to_ne_bytes());
        m.pad(8); // the body starts on an 8-byte boundary
        m.body = m.b.len();
        m
    }
}

enum BusError {
    /// the peer answered with a D-Bus error: its name and message
    Reply(String),
    /// the connection itself failed
    Io(String),
}

impl BusError {
    fn text(self) -> String {
        match self {
            BusError::Reply(s) | BusError::Io(s) => s,
        }
    }
}

struct Bus {
    sock: UnixStream,
    serial: u32,
    /// the peer marshals in the other byte order
    swap: bool,
}

/// One address out of DBUS_SESSION_BUS_ADDRESS: a semicolon-separated list of
/// comma-separated key=value pairs, the first of which carries the transport.
/// Returns the socket path and whether it is abstract.
fn bus_addr_path(addr: &str) -> Option<(String, bool)> {
    for pair in addr.split([',', ';']) {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let key = key.strip_prefix("unix:").unwrap_or(key);
        if !value.is_empty() && (key == "path" || key == "abstract") {
            return Some((value.to_string(), key == "abstract"));
        }
    }
    None
}

impl Bus {
    /// Connect to the session bus and get through the SASL handshake. Returns
    /// None when there is no bus to talk to, which is not by itself an error:
    /// the caller has a better message to print than we do.
    fn connect() -> Result<Option<Bus>> {
        let (path, abstract_) = match std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .ok()
            .and_then(|addr| bus_addr_path(&addr))
        {
            Some(found) => found,
            None => match std::env::var("XDG_RUNTIME_DIR") {
                Ok(rt) => (format!("{rt}/bus"), false),
                Err(_) => return Ok(None),
            },
        };
        let sock = if abstract_ {
            SocketAddr::from_abstract_name(path.as_bytes())
                .and_then(|addr| UnixStream::connect_addr(&addr))
        } else {
            UnixStream::connect(&path)
        };
        let Ok(sock) = sock else {
            return Ok(None);
        };
        let mut bus = Bus {
            sock,
            serial: 0,
            swap: false,
        };

        // SASL EXTERNAL: the kernel already told the bus who we are, so the
        // only "credential" is our uid, in hex, after a leading NUL byte.
        let hex: String = unsafe { libc::getuid() }
            .to_string()
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect();
        bus.write(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let line = bus.line()?;
        if !line.starts_with("OK") {
            return Err(format!("the session bus rejected our credentials: {line}"));
        }
        bus.write(b"BEGIN\r\n")?;

        // nothing is routed before Hello
        let mut m = DbusMsg::call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "Hello",
            "",
        );
        let serial = bus.send(&mut m)?;
        bus.wait(serial)
            .map_err(|e| format!("the session bus refused Hello: {}", e.text()))?;
        Ok(Some(bus))
    }

    fn write(&mut self, buf: &[u8]) -> Result<()> {
        self.sock
            .write_all(buf)
            .map_err(|e| format!("write to the session bus failed: {e}"))
    }

    fn read(&mut self, n: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0; n];
        self.sock.read_exact(&mut buf).map_err(|e| match e.kind() {
            ErrorKind::UnexpectedEof => "the session bus closed the connection".to_string(),
            _ => format!("read from the session bus failed: {e}"),
        })?;
        Ok(buf)
    }

    /// one line of the SASL exchange, without its line ending
    fn line(&mut self) -> Result<String> {
        let mut out = Vec::new();
        loop {
            let c = self.read(1)?[0];
            if c == b'\n' {
                if out.last() == Some(&b'\r') {
                    out.pop();
                }
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            out.push(c);
        }
    }

    fn send(&mut self, m: &mut DbusMsg) -> Result<u32> {
        let blen = (m.b.len() - m.body) as u32;
        self.serial += 1;
        m.b[4..8].copy_from_slice(&blen.to_ne_bytes());
        m.b[8..12].copy_from_slice(&self.serial.to_ne_bytes());
        self.write(&m.b)?;
        Ok(self.serial)
    }

    fn get32(&self, p: &[u8], off: usize) -> u32 {
        let v = get_u32(p, off);
        if self.swap {
            v.swap_bytes()
        } else {
            v
        }
    }

    /// the string at *off, advancing past it; None if it runs off the end
    fn take_str(&self, buf: &[u8], off: &mut usize) -> Option<String> {
        *off = (*off + 3) & !3;
        if *off + 4 > buf.len() {
            return None;
        }
        let l = self.get32(buf, *off) as usize;
        *off += 4;
        if l >= buf.len() - *off {
            return None; // the NUL must fit too
        }
        let s = String::from_utf8_lossy(&buf[*off..*off + l]).into_owned();
        *off += l + 1;
        Some(s)
    }

    /// Read messages until the reply to `serial` arrives; signals and anything
    /// else on the bus are dropped. A method return yields its first argument
    /// (always a string in the calls we make, or nothing), an error reply its
    /// name and message.
    fn wait(&mut self, serial: u32) -> std::result::Result<String, BusError> {
        loop {
            let hdr = self.read(16).map_err(BusError::Io)?;
            self.swap = hdr[0] != HOST_ORDER;
            if hdr[3] != 1 {
                return Err(BusError::Io(format!(
                    "the session bus speaks D-Bus protocol version {}",
                    hdr[3]
                )));
            }
            let (blen, flen) = (self.get32(&hdr, 4) as usize, self.get32(&hdr, 12) as usize);
            let fpad = (8 - (flen & 7)) & 7;
            let buf = self.read(flen + fpad + blen).map_err(BusError::Io)?;
            let (fields, body) = (&buf[..flen], &buf[flen + fpad..]);

            // walk the header fields far enough to see which reply this is
            let (mut ename, mut rserial) = (String::new(), 0);
            let mut o = 0;
            while o < flen {
                o = (o + 7) & !7;
                if o + 4 > flen {
                    break;
                }
                let (code, slen, t) = (fields[o], fields[o + 1] as usize, fields[o + 2]);
                o += slen + 3;
                match t {
                    b's' | b'o' => {
                        let Some(s) = self.take_str(fields, &mut o) else {
                            break;
                        };
                        if code == D_ERRNAME {
                            ename = s;
                        }
                    }
                    b'g' => {
                        if o >= flen {
                            break;
                        }
                        o += fields[o] as usize + 2;
                    }
                    b'u' => {
                        o = (o + 3) & !3;
                        if o + 4 > flen {
                            break;
                        }
                        let v = self.get32(fields, o);
                        o += 4;
                        if code == D_REPLYSERIAL {
                            rserial = v;
                        }
                    }
                    _ => break, // nothing else turns up in these headers
                }
            }
            if rserial != serial {
                continue; // not ours
            }

            let mut o = 0;
            let arg = if blen > 0 {
                self.take_str(body, &mut o)
            } else {
                None
            }
            .unwrap_or_default();
            if hdr[1] == D_RETURN {
                return Ok(arg);
            }
            return Err(BusError::Reply(if arg.is_empty() {
                ename
            } else {
                format!("{ename}: {arg}")
            }));
        }
    }
}

struct RemoteDesktop {
    bus: Bus,
    service: &'static str,
    /// object path of the session we created, and its interface name
    path: String,
    iface: String,
    warned: bool,
}

impl KeyEmitter for RemoteDesktop {
    fn key(&mut self, code: u32, state: u32) -> Result<()> {
        let mut m = DbusMsg::call(
            self.service,
            &self.path,
            &self.iface,
            "NotifyKeyboardKeycode",
            "ub",
        );
        m.u32(code);
        m.u32((state != 0) as u32);
        // Waiting for the reply costs a round trip per key, but it is what
        // surfaces a refusal and keeps us from filling the bus socket.
        let serial = self.bus.send(&mut m)?;
        match self.bus.wait(serial) {
            Ok(_) => Ok(()),
            Err(BusError::Reply(e)) => {
                if !self.warned {
                    warn!("the compositor rejected a key event: {e}");
                    self.warned = true;
                }
                Ok(())
            }
            Err(BusError::Io(e)) => Err(e),
        }
    }

    /// End the session rather than leaving it to the socket being closed.
    fn done(&mut self) -> Result<()> {
        let mut m = DbusMsg::call(self.service, &self.path, &self.iface, "Stop", "");
        let serial = self.bus.send(&mut m)?;
        let _ = self.bus.wait(serial);
        Ok(())
    }
}

/// Ok(false) when nothing on the bus answers, which in auto mode just means
/// this is not one of those desktops.
fn run_remote_desktop(wl: &mut Wayland, cmds: &[Cmd]) -> Result<bool> {
    let Some(mut bus) = Bus::connect()? else {
        debug!("no session bus to ask about remote desktop");
        return Ok(false);
    };

    // An unowned name means only that this is not that desktop, so keep
    // looking; anything else is a real refusal and worth reporting.
    let mut session = None;
    for (service, path) in RD_FLAVORS {
        debug!("asking {service} for a session");
        let mut m = DbusMsg::call(service, path, service, "CreateSession", "");
        let serial = bus.send(&mut m)?;
        match bus.wait(serial) {
            Ok(path) => {
                session = Some((service, path));
                break;
            }
            Err(BusError::Reply(e)) => {
                debug!("  {e}");
                if !["ServiceUnknown", "NameHasNoOwner", "NoReply"]
                    .iter()
                    .any(|s| e.contains(s))
                {
                    return Err(format!("{service} refused a remote-desktop session: {e}"));
                }
            }
            Err(BusError::Io(e)) => return Err(e),
        }
    }
    let Some((service, path)) = session else {
        return Ok(false);
    };
    if path.is_empty() {
        return Err(format!("{service} returned an empty session path"));
    }
    let iface = format!("{service}.Session");

    let mut m = DbusMsg::call(service, &path, &iface, "Start", "");
    let serial = bus.send(&mut m)?;
    bus.wait(serial)
        .map_err(|e| format!("{service} would not start the session: {}", e.text()))?;
    debug!("using remote-desktop ({service}, session {path})");

    let keymap = fetch_seat_keymap(wl)?;
    let mut emit = RemoteDesktop {
        bus,
        service,
        path,
        iface,
        warned: false,
    };
    let mut typist = Keycodes {
        emit: &mut emit,
        keymap: &keymap,
        held_mods: 0,
    };
    run_cmds(&mut typist, cmds)?;
    Ok(true)
}

/* ------------------------------------------- backend D: uinput, via ydotoold */
/*
 * Where no compositor protocol answers, ydotoold might: it holds a uinput
 * device open and replays whatever arrives on its socket into the kernel, so
 * the events come back around as if a keyboard had produced them. That reaches
 * any compositor at all, which is why it is worth having, and it costs a root
 * daemon, which is why it is last. Keycodes still land on the session's own
 * keymap, so the layout limit is the one the backends above have.
 */

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const SYN_REPORT: u16 = 0;

struct Ydotool(UnixDatagram);

impl Ydotool {
    fn try_path(path: &str) -> Option<Ydotool> {
        let sock = UnixDatagram::unbound().ok()?;
        if sock.connect(path).is_ok() {
            debug!("using uinput (ydotoold, socket {path})");
            return Some(Ydotool(sock));
        }
        debug!("no ydotoold on {path}");
        None
    }

    /// Where the daemon puts its socket: under XDG_RUNTIME_DIR when it runs as
    /// the user, in /tmp when it runs as root. ydotool's own client picks one
    /// of those and gives up; trying both costs nothing and finds a root
    /// daemon from inside a user session.
    fn connect() -> Option<Ydotool> {
        let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        if let Some(path) = var("YDOTOOL_SOCKET") {
            return Self::try_path(&path);
        }
        var("XDG_RUNTIME_DIR")
            .and_then(|rt| Self::try_path(&format!("{rt}/.ydotool_socket")))
            .or_else(|| Self::try_path("/tmp/.ydotool_socket"))
    }

    fn emit(&self, ty: u16, code: u16, value: i32) -> Result<()> {
        // struct input_event, spelled out: a timestamp of two kernel longs,
        // which is what makes the size match ydotoold's on 32- and 64-bit
        let mut ev = vec![0u8; 2 * std::mem::size_of::<libc::c_long>()];
        ev.extend_from_slice(&ty.to_ne_bytes());
        ev.extend_from_slice(&code.to_ne_bytes());
        ev.extend_from_slice(&value.to_ne_bytes());
        loop {
            match self.0.send(&ev) {
                Ok(n) if n == ev.len() => return Ok(()),
                Ok(_) => return Err("write to the ydotoold socket failed".into()),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("write to the ydotoold socket failed: {e}")),
            }
        }
    }
}

impl KeyEmitter for Ydotool {
    /// a key event, and the SYN_REPORT that makes the kernel act on it
    fn key(&mut self, code: u32, state: u32) -> Result<()> {
        self.emit(EV_KEY, code as u16, (state != 0) as i32)?;
        self.emit(EV_SYN, SYN_REPORT, 0)
    }

    fn done(&mut self) -> Result<()> {
        Ok(())
    }
}

fn run_ydotool(wl: &mut Wayland, cmds: &[Cmd]) -> Result<bool> {
    let Some(mut yd) = Ydotool::connect() else {
        return Ok(false);
    };
    let keymap = fetch_seat_keymap(wl)?;
    let mut typist = Keycodes {
        emit: &mut yd,
        keymap: &keymap,
        held_mods: 0,
    };
    run_cmds(&mut typist, cmds)?;
    Ok(true)
}

/* --------------------------------------------------- backend E: X11/XTEST */
/*
 * Spoken straight over the display socket, like the Wayland side: no libX11,
 * no libXtst. XTEST fakes *keycodes*, not keysyms, so a keysym is typed on
 * whichever key of the current layout carries it, adding Shift when the layout
 * only offers it shifted. Whatever the layout cannot produce at all is parked
 * on a keycode it leaves empty (ChangeKeyboardMapping) and the mapping is put
 * back on drop, so the backend can still type any character on any layout.
 *
 * Remapping is the slow path: it makes the server broadcast a MappingNotify
 * and every client re-read the keyboard, so it is worth pausing around, and
 * worth avoiding. Ordinary text never triggers it.
 */

/// core request opcodes we use
const X_QUERY_POINTER: u8 = 38;
const X_GET_INPUT_FOCUS: u8 = 43;
const X_QUERY_EXTENSION: u8 = 98;
const X_CHANGE_KEYBOARD_MAPPING: u8 = 100;
const X_GET_KEYBOARD_MAPPING: u8 = 101;
/// XTEST's minor opcode, and the event types FakeInput takes
const XT_FAKE_INPUT: u8 = 2;
const X_KEY_PRESS: u8 = 2;
const X_KEY_RELEASE: u8 = 3;

/// Caps Lock, in a KEYBUTMASK
const X_LOCK_MASK: u16 = 2;
/// Xauthority family for local connections
const X_FAMILY_LOCAL: u16 = 256;
/// grace for clients to re-read a changed keymap, in ms
const REMAP_MS: u64 = 50;

/// a core request: opcode, the byte after it, the length in words, the rest
fn xreq(op: u8, b1: u8, words: u16, rest: &[u8]) -> Vec<u8> {
    let mut r = vec![op, b1];
    r.extend_from_slice(&words.to_ne_bytes());
    r.extend_from_slice(rest);
    r
}

fn hostname() -> Vec<u8> {
    let mut buf = [0u8; 256];
    if unsafe { libc::gethostname(buf.as_mut_ptr() as *mut _, buf.len() - 1) } != 0 {
        return Vec::new();
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf[..len].to_vec()
}

struct X11 {
    sock: File,
    /// XTEST's major opcode on this server
    xtest_op: u8,
    /// root window of the first screen
    root: u32,
    /// keyboard mapping geometry
    min_kc: u8,
    max_kc: u8,
    syms: usize,
    /// the mapping as we found it, never modified
    map: Vec<u32>,
    /// keycodes we rebound, ascending
    touched: Vec<u8>,
    /// keycode holding each collected slot
    slotkc: Vec<u8>,
    /// keycode rebound on demand when slots run out, and what it holds now
    spill: u8,
    spill_ks: u32,
    slots: Slots,
    held_mods: u32,
}

impl X11 {
    /// Connect to $DISPLAY, "[host]:display[.screen]". An empty or "unix" host
    /// means the local socket, anything else (including "localhost", which is
    /// how ssh -X forwarding presents itself) means TCP.
    fn connect(disp: &str, slots: Slots) -> Result<X11> {
        let (host, rest) = disp
            .rsplit_once(':')
            .ok_or_else(|| format!("malformed DISPLAY '{disp}'"))?;
        let dnum: i32 = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        let cannot = || format!("cannot connect to the X display {disp}");
        let sock: OwnedFd = if host.is_empty() || host == "unix" {
            let path = format!("/tmp/.X11-unix/X{dnum}");
            // the abstract socket first, the way Xlib probes for it on Linux
            SocketAddr::from_abstract_name(path.as_bytes())
                .and_then(|addr| UnixStream::connect_addr(&addr))
                .or_else(|_| UnixStream::connect(&path))
                .map_err(|_| cannot())?
                .into()
        } else {
            TcpStream::connect((host, 6000 + dnum as u16))
                .map_err(|_| cannot())?
                .into()
        };
        let mut x = X11 {
            sock: File::from(sock),
            xtest_op: 0,
            root: 0,
            min_kc: 0,
            max_kc: 0,
            syms: 0,
            map: Vec::new(),
            touched: Vec::new(),
            slotkc: Vec::new(),
            spill: 0,
            spill_ks: 0,
            slots,
            held_mods: 0,
        };
        x.setup(&X11::cookie(dnum))?;
        x.query_xtest()?;
        x.read_map()?;
        Ok(x)
    }

    fn write(&mut self, buf: &[u8]) -> Result<()> {
        self.sock
            .write_all(buf)
            .map_err(|e| format!("write to the X server failed: {e}"))
    }

    fn read(&mut self, n: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0; n];
        self.sock.read_exact(&mut buf).map_err(|e| match e.kind() {
            ErrorKind::UnexpectedEof => "the X server closed the connection".to_string(),
            _ => format!("read from the X server failed: {e}"),
        })?;
        Ok(buf)
    }

    /// Read packets until the reply we are waiting for turns up. Events arrive
    /// unasked (every ChangeKeyboardMapping makes the server broadcast a
    /// MappingNotify) and are discarded; errors are fatal. Returns the 32-byte
    /// header and whatever data follows it.
    fn reply(&mut self) -> Result<(Vec<u8>, Vec<u8>)> {
        loop {
            let hdr = self.read(32)?;
            match hdr[0] {
                0 => {
                    return Err(format!(
                        "X error {} on request {}.{}",
                        hdr[1],
                        hdr[10],
                        get_u16(&hdr, 8)
                    ))
                }
                1 => {
                    let extra = self.read(get_u32(&hdr, 4) as usize * 4)?;
                    return Ok((hdr, extra));
                }
                t if t & 0x7f == 35 => {
                    self.read(get_u32(&hdr, 4) as usize * 4)?; // GenericEvent
                }
                _ => {}
            }
        }
    }

    /// Block until the server has processed everything sent so far.
    /// GetInputFocus is the cheapest request that carries a reply, which is
    /// what XSync uses.
    fn sync(&mut self) -> Result<()> {
        self.write(&xreq(X_GET_INPUT_FOCUS, 0, 1, &[]))?;
        self.reply().map(|_| ())
    }

    /// Find this display's MIT-MAGIC-COOKIE-1. A missing or unreadable file is
    /// not an error: servers with authorization turned off let us in anyway.
    fn cookie(dnum: i32) -> Vec<u8> {
        let path = match std::env::var("XAUTHORITY") {
            Ok(path) if !path.is_empty() => path,
            _ => match std::env::var("HOME") {
                Ok(home) => format!("{home}/.Xauthority"),
                Err(_) => return Vec::new(),
            },
        };
        let Ok(data) = std::fs::read(path) else {
            return Vec::new();
        };
        // Xauthority is big-endian: u16 fields, and u16-length-prefixed byte fields
        fn u16_at(d: &[u8], off: usize) -> Option<u16> {
            Some(u16::from_be_bytes([*d.get(off)?, *d.get(off + 1)?]))
        }
        fn field<'a>(d: &'a [u8], off: &mut usize) -> Option<&'a [u8]> {
            let n = u16_at(d, *off)? as usize;
            let f = d.get(*off + 2..*off + 2 + n)?;
            *off += 2 + n;
            Some(f)
        }
        let host = hostname();
        let want = dnum.to_string();
        let (mut best, mut cookie) = (0, Vec::new());
        let mut off = 0;
        while best < 2 {
            let Some(fam) = u16_at(&data, off) else {
                break;
            };
            off += 2;
            let (Some(addr), Some(num), Some(name), Some(cdata)) = (
                field(&data, &mut off),
                field(&data, &mut off),
                field(&data, &mut off),
                field(&data, &mut off),
            ) else {
                break;
            };
            if name != b"MIT-MAGIC-COOKIE-1" || (!num.is_empty() && num != want.as_bytes()) {
                continue;
            }
            // an entry naming this host beats a generic one, but either will do
            let score = if fam == X_FAMILY_LOCAL && addr == host {
                2
            } else {
                1
            };
            if score > best {
                best = score;
                cookie = cdata.to_vec();
            }
        }
        cookie
    }

    /// The connection handshake. The server answers in whatever byte order we
    /// ask for, so every field after this one is plain host-order.
    fn setup(&mut self, cookie: &[u8]) -> Result<()> {
        let proto: &[u8] = if cookie.is_empty() {
            b""
        } else {
            b"MIT-MAGIC-COOKIE-1"
        };
        let mut req = vec![HOST_ORDER, 0];
        req.extend_from_slice(&11u16.to_ne_bytes()); // protocol version 11.0
        req.extend_from_slice(&0u16.to_ne_bytes());
        req.extend_from_slice(&(proto.len() as u16).to_ne_bytes());
        req.extend_from_slice(&(cookie.len() as u16).to_ne_bytes());
        req.extend_from_slice(&[0, 0]);
        for part in [proto, cookie] {
            req.extend_from_slice(part);
            while !req.len().is_multiple_of(4) {
                req.push(0);
            }
        }
        self.write(&req)?;

        let hdr = self.read(8)?;
        let body = self.read(get_u16(&hdr, 6) as usize * 4)?;
        if hdr[0] != 1 {
            let reason = &body[..(hdr[1] as usize).min(body.len())];
            return Err(format!(
                "the X server refused the connection: {}",
                String::from_utf8_lossy(reason)
            ));
        }
        if body.len() < 32 {
            return Err("malformed X server setup reply".into());
        }
        self.min_kc = body[26];
        self.max_kc = body[27];
        if self.max_kc < self.min_kc {
            return Err("the X server reports an empty keycode range".into());
        }
        if body[20] == 0 {
            return Err("the X server reports no screens".into());
        }
        // the screens follow the vendor string and the pixmap formats; a
        // screen opens with its root window, which is all we need from there
        let off = 32 + ((get_u16(&body, 16) as usize + 3) & !3) + body[21] as usize * 8;
        if off + 4 > body.len() {
            return Err("malformed X server setup reply".into());
        }
        self.root = get_u32(&body, off);
        Ok(())
    }

    fn query_xtest(&mut self) -> Result<()> {
        let name = b"XTEST";
        let padded = (name.len() + 3) & !3;
        let mut req = xreq(X_QUERY_EXTENSION, 0, (2 + padded / 4) as u16, &[]);
        req.extend_from_slice(&(name.len() as u16).to_ne_bytes());
        req.extend_from_slice(&[0, 0]);
        req.extend_from_slice(name);
        req.resize(8 + padded, 0);
        self.write(&req)?;
        let (hdr, _) = self.reply()?;
        if hdr[8] == 0 {
            return Err("this X server has no XTEST extension; input cannot be faked".into());
        }
        self.xtest_op = hdr[9];
        debug!("XTEST is present, major opcode {}", self.xtest_op);
        Ok(())
    }

    fn read_map(&mut self) -> Result<()> {
        let count = (self.max_kc - self.min_kc) as usize + 1;
        self.write(&xreq(
            X_GET_KEYBOARD_MAPPING,
            0,
            2,
            &[self.min_kc, count as u8, 0, 0],
        ))?;
        let (hdr, extra) = self.reply()?;
        self.syms = hdr[1] as usize;
        if self.syms < 1 || extra.is_empty() {
            return Err("the X server returned an empty keyboard mapping".into());
        }
        if get_u32(&hdr, 4) as usize != count * self.syms {
            return Err("malformed keyboard mapping reply".into());
        }
        self.map = extra
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_ne_bytes(*c))
            .collect();
        Ok(())
    }

    /// the keysym at shift level `i` of keycode `kc`, as the server reported it
    fn sym(&self, kc: u8, i: usize) -> u32 {
        self.map[(kc - self.min_kc) as usize * self.syms + i]
    }

    /// The keycode whose plain, unmodified keysym is ks, or None if the layout
    /// has no such key. Only used for the modifier keys, which we press for real.
    fn find_kc(&self, ks: u32) -> Option<u8> {
        (self.min_kc..=self.max_kc).find(|&kc| self.sym(kc, 0) == ks)
    }

    /// Where the current layout puts ks: the keycode, and whether it sits at
    /// the shifted level. None when the layout cannot produce it, which sends
    /// the caller off to borrow a keycode instead. Only the unshifted and
    /// shifted levels of the first group are considered; deeper levels need a
    /// modifier or group switch this backend does not drive, and borrowing is
    /// both simpler and more predictable than guessing at them.
    fn layout_kc(&self, ks: u32) -> Option<(u8, bool)> {
        if ks == 0 {
            return None;
        }
        if let Some(kc) = self.find_kc(ks) {
            return Some((kc, false)); // unshifted: no modifier needed
        }
        if self.syms >= 2 {
            if let Some(kc) = (self.min_kc..=self.max_kc).find(|&kc| self.sym(kc, 1) == ks) {
                return Some((kc, true));
            }
        }
        None
    }

    /// rebind a contiguous run of keycodes, `n` of them, `self.syms` keysyms each
    fn change_map(&mut self, first: u8, n: u8, syms: &[u32]) -> Result<()> {
        let words = 2 + n as usize * self.syms;
        let mut req = xreq(
            X_CHANGE_KEYBOARD_MAPPING,
            n,
            words as u16,
            &[first, self.syms as u8, 0, 0],
        );
        for s in syms {
            req.extend_from_slice(&s.to_ne_bytes());
        }
        self.write(&req)
    }

    /// Put every keycode we rebound back the way the server had it. Runs on
    /// drop, so a failure partway through still leaves the user's keyboard
    /// alone.
    fn restore(&mut self) {
        let touched = std::mem::take(&mut self.touched);
        if touched.is_empty() {
            return;
        }
        // Let whatever has focus translate the keycodes we just sent before the
        // mapping under them moves again; clients read the map on their own clock.
        nap(REMAP_MS * 2);
        let mut i = 0;
        while i < touched.len() {
            let mut run = 1;
            while i + run < touched.len() && touched[i + run] as usize == touched[i] as usize + run
            {
                run += 1;
            }
            let mut syms = Vec::with_capacity(run * self.syms);
            for &kc in &touched[i..i + run] {
                for c in 0..self.syms {
                    syms.push(self.sym(kc, c));
                }
            }
            if self.change_map(touched[i], run as u8, &syms).is_err() {
                return;
            }
            i += run;
        }
        let _ = self.sync();
    }

    /// Park every collected keysym on a keycode the layout does not use,
    /// repeating it across the full width of the row. Filling every column
    /// makes the key immune to Shift, Caps Lock and the active layout group,
    /// and keeping the row exactly as wide as the server's own
    /// keysyms_per_keycode means the server never has to resize the map, which
    /// would rewrite (and subtly damage) rows we never asked to touch.
    fn bind_slots(&mut self) -> Result<()> {
        // Drop the keysyms the layout can already produce: those are typed on
        // their own keys and cost no remapping, which is the common case and
        // the whole reason plain text needs no keymap change at all.
        let slots = std::mem::take(&mut self.slots.0);
        self.slots.0 = slots
            .into_iter()
            .filter(|&ks| self.layout_kc(ks).is_none())
            .collect();
        let nslots = self.slots.0.len();

        let spare: Vec<u8> = (self.min_kc..=self.max_kc)
            .filter(|&kc| (0..self.syms).all(|i| self.sym(kc, i) == 0))
            .collect();

        let nbind = if nslots <= spare.len() {
            self.spill = 0;
            nslots
        } else if let Some((&last, rest)) = spare.split_last() {
            self.spill = last;
            rest.len()
        } else {
            self.spill = self.max_kc; // borrow one
            0
        };

        self.slotkc = vec![0; nslots];
        let mut i = 0;
        while i < nbind {
            let mut run = 1;
            while i + run < nbind && spare[i + run] as usize == spare[i] as usize + run {
                run += 1;
            }
            let mut syms = Vec::with_capacity(run * self.syms);
            for &ks in &self.slots.0[i..i + run] {
                syms.extend(std::iter::repeat_n(ks, self.syms));
            }
            self.slotkc[i..i + run].copy_from_slice(&spare[i..i + run]);
            self.touched.extend_from_slice(&spare[i..i + run]);
            self.change_map(spare[i], run as u8, &syms)?;
            i += run;
        }
        if self.spill != 0 {
            self.touched.push(self.spill);
        }

        if !self.touched.is_empty() {
            debug!(
                "borrowed {} keycode(s) for characters the layout cannot produce",
                self.touched.len()
            );
            self.sync()?;
            nap(REMAP_MS); // toolkits re-read the map when our MappingNotify lands
        }
        Ok(())
    }

    /// The keycode to press for ks, and whether Shift is needed: its own key on
    /// the layout where there is one, otherwise a borrowed one, rebinding the
    /// spill key if the layout had fewer free keycodes than we needed slots.
    /// Keycode 0 means there is nothing left to press.
    fn kc_for(&mut self, ks: u32) -> Result<(u8, bool)> {
        if let Some(found) = self.layout_kc(ks) {
            return Ok(found);
        }
        let slot = self.slots.slot_for(ks) as usize - 1;
        if let Some(&kc) = self.slotkc.get(slot).filter(|&&kc| kc != 0) {
            return Ok((kc, false)); // borrowed keys carry the keysym at every level
        }
        if self.spill == 0 {
            warn!("no free keycode left for this key; skipped");
            return Ok((0, false));
        }
        if self.spill_ks != ks {
            let row = vec![ks; self.syms];
            self.change_map(self.spill, 1, &row)?;
            self.sync()?;
            nap(REMAP_MS);
            self.spill_ks = ks;
        }
        Ok((self.spill, false))
    }

    fn key(&mut self, kc: u8, down: bool) -> Result<()> {
        if kc == 0 {
            return Ok(());
        }
        let ty = if down { X_KEY_PRESS } else { X_KEY_RELEASE };
        let mut req = xreq(self.xtest_op, XT_FAKE_INPUT, 9, &[ty, kc]);
        // time 0 asks for no server-side delay, root None means the current
        // screen, and device 0 means the core keyboard
        req.resize(36, 0);
        self.write(&req)
    }

    fn tap(&mut self, kc: u8) -> Result<()> {
        self.key(kc, true)?;
        nap(HOLD_MS);
        self.key(kc, false)?;
        nap(HOLD_MS);
        Ok(())
    }

    /// A modifier's keycode: a real key from the layout where there is one,
    /// else a scratch key holding the keysym, which the server's compatibility
    /// map turns back into the modifier.
    fn mod_kc(&mut self, m: u32) -> Result<u8> {
        let keysyms = mod_keysyms(m);
        if let Some(kc) = keysyms.iter().find_map(|&ks| self.find_kc(ks)) {
            return Ok(kc);
        }
        match keysyms.first() {
            Some(&ks) => Ok(self.kc_for(ks)?.0),
            None => Ok(0),
        }
    }

    /// Modifiers with no key of their own on this layout need a slot as well.
    fn collect_mod_slots(&mut self, cmds: &[Cmd]) {
        for c in cmds {
            let (Cmd::ModPress(m) | Cmd::ModRelease(m)) = c else {
                continue;
            };
            let keysyms = mod_keysyms(*m);
            if let Some(&first) = keysyms.first() {
                if !keysyms.iter().any(|&ks| self.find_kc(ks).is_some()) {
                    self.slots.slot_for(first);
                }
            }
        }
    }
}

impl Typist for X11 {
    /// Type one keysym: press the key that carries it, holding Shift for the
    /// run of the stroke when the layout only offers it shifted and the caller
    /// is not already holding Shift itself.
    fn stroke(&mut self, ks: u32) -> Result<()> {
        let (kc, shift) = self.kc_for(ks)?;
        if kc == 0 {
            return Ok(());
        }
        let skc = if shift && self.held_mods & SHIFT == 0 {
            self.mod_kc(SHIFT)?
        } else {
            0
        };
        if skc != 0 {
            self.key(skc, true)?;
        }
        self.tap(kc)?;
        if skc != 0 {
            self.key(skc, false)?;
        }
        Ok(())
    }

    fn modifier(&mut self, m: u32, press: bool) -> Result<()> {
        let kc = self.mod_kc(m)?;
        if kc == 0 {
            warn!("this modifier has no key on the X server; skipped");
            return Ok(());
        }
        if m == CAPS {
            self.tap(kc)?; // a lock toggle, not a hold
        } else {
            self.key(kc, press)?;
        }
        if press {
            self.held_mods |= m;
        } else {
            self.held_mods &= !m;
        }
        Ok(())
    }

    fn caps_lock_on(&mut self) -> Result<bool> {
        let mut req = xreq(X_QUERY_POINTER, 0, 2, &[]);
        req.extend_from_slice(&self.root.to_ne_bytes());
        self.write(&req)?;
        let (hdr, _) = self.reply()?;
        Ok(get_u16(&hdr, 24) & X_LOCK_MASK != 0)
    }

    fn done(&mut self) -> Result<()> {
        self.sync()
    }
}

impl Drop for X11 {
    fn drop(&mut self) {
        self.restore();
    }
}

/// the keysyms that carry each modifier, best first
fn mod_keysyms(m: u32) -> &'static [u32] {
    match m {
        SHIFT => &[0xffe1, 0xffe2],         // Shift_L, Shift_R
        CTRL => &[0xffe3, 0xffe4],          // Control_L, Control_R
        ALT => &[0xffe9, 0xffea, 0xffe7],   // Alt_L, Alt_R, Meta_L
        LOGO => &[0xffeb, 0xffec],          // Super_L, Super_R
        ALTGR => &[0xfe03, 0xff7e, 0xffea], // ISO_Level3_Shift, Mode_switch, Alt_R
        CAPS => &[KS_CAPS_LOCK],
        _ => &[],
    }
}

fn run_x11(cmds: &[Cmd]) -> Result<()> {
    let disp = std::env::var("DISPLAY").unwrap_or_default();
    debug!("using xtest (X display {disp})");
    let mut x = X11::connect(&disp, Slots::collect(cmds))?;
    x.collect_mod_slots(cmds);
    x.bind_slots()?; // drops what the layout can type
    run_cmds(&mut x, cmds)
    // dropping `x` restores the keymap
}

/* -------------------------------------------------------- protocol choice */

#[derive(Clone, Copy, PartialEq)]
enum Protocol {
    Auto,
    VirtualKeyboard,
    FakeInput,
    RemoteDesktop,
    Uinput,
    Xtest,
}

/// HANDY_TYPING_PROTOCOL pins the choice instead of letting the environment
/// decide, which is what to reach for when the automatic answer is the wrong
/// one. An unusable choice is an error, not a fallback: asking for something
/// specific and silently getting something else would defeat the point.
fn forced_protocol() -> Result<Protocol> {
    let wanted = std::env::var("HANDY_TYPING_PROTOCOL").unwrap_or_default();
    let protocol = match wanted.to_ascii_lowercase().as_str() {
        "" => return Ok(Protocol::Auto),
        "auto" => Protocol::Auto,
        "virtual-keyboard" => Protocol::VirtualKeyboard,
        "fake-input" => Protocol::FakeInput,
        "remote-desktop" => Protocol::RemoteDesktop,
        "uinput" => Protocol::Uinput,
        "xtest" => Protocol::Xtest,
        _ => {
            return Err(format!(
                "HANDY_TYPING_PROTOCOL: unknown protocol '{wanted}'. Use one of: auto, \
                 virtual-keyboard, fake-input, remote-desktop, uinput, xtest."
            ))
        }
    };
    debug!("HANDY_TYPING_PROTOCOL pins the choice to {wanted}");
    Ok(protocol)
}

/// Run a sequence of actions against whichever session the environment
/// describes: a Wayland compositor when WAYLAND_DISPLAY is set and reachable,
/// otherwise the X11 server named by DISPLAY.
fn run(cmds: &[Cmd]) -> Result<()> {
    let want = forced_protocol()?;
    let wd = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    let xd = std::env::var("DISPLAY").unwrap_or_default();
    let (have_wl, have_x11) = (!wd.is_empty(), !xd.is_empty());
    debug!(
        "WAYLAND_DISPLAY={}, DISPLAY={}",
        if have_wl { wd.as_str() } else { "(unset)" },
        if have_x11 { xd.as_str() } else { "(unset)" }
    );

    if want == Protocol::Xtest || (want == Protocol::Auto && !have_wl) {
        if !have_x11 {
            return Err(if want == Protocol::Xtest {
                "HANDY_TYPING_PROTOCOL=xtest, but DISPLAY is not set"
            } else {
                "no display server found: neither WAYLAND_DISPLAY nor DISPLAY is set"
            }
            .into());
        }
        return run_x11(cmds);
    }

    // Everything else rides on the Wayland connection, uinput included: that
    // backend presses keycodes, so it still needs the compositor's keymap to
    // know which one carries each character.
    let mut wl = match Wayland::connect() {
        Ok(wl) => wl,
        Err(e) if want == Protocol::Auto && have_x11 => {
            warn!("{e}; falling back to X11");
            return run_x11(cmds);
        }
        Err(e) => return Err(e),
    };
    wl.get_globals()?;
    debug!(
        "compositor offers: virtual-keyboard {}, fake_input {}, wl_seat {}",
        yesno(wl.vkm.is_some()),
        yesno(wl.fake.is_some()),
        yesno(wl.seat.is_some())
    );

    match want {
        Protocol::VirtualKeyboard => {
            if wl.vkm.is_none() {
                return Err(
                    "HANDY_TYPING_PROTOCOL=virtual-keyboard, but this compositor does not offer it"
                        .into(),
                );
            }
            return run_virtual_keyboard(&mut wl, cmds);
        }
        Protocol::FakeInput => return run_fake_input(wl, cmds),
        Protocol::RemoteDesktop => {
            return match run_remote_desktop(&mut wl, cmds)? {
                true => Ok(()),
                false => Err(
                    "HANDY_TYPING_PROTOCOL=remote-desktop, but nothing answered on the session bus"
                        .into(),
                ),
            }
        }
        Protocol::Uinput => {
            return match run_ydotool(&mut wl, cmds)? {
                true => Ok(()),
                false => Err("HANDY_TYPING_PROTOCOL=uinput, but ydotoold is not reachable".into()),
            }
        }
        Protocol::Auto | Protocol::Xtest => {}
    }

    // KWin hands out fake_input only after a first run installs the .desktop
    // file that asks for it, so trust the desktop's own name over the absence
    // of the global.
    if wl.vkm.is_some() {
        run_virtual_keyboard(&mut wl, cmds)
    } else if wl.fake.is_some() || crate::utils::is_kde_plasma() {
        run_fake_input(wl, cmds)
    } else if run_remote_desktop(&mut wl, cmds)? || run_ydotool(&mut wl, cmds)? {
        Ok(())
    } else {
        Err(NO_INPUT_PROTOCOL.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An AppImage's AppRun prepends its own tree to XDG_DATA_DIRS, which would
    /// send the KService cache rebuild to a file KWin does not read.
    #[test]
    fn session_data_dirs_undoes_an_appimage() {
        let session = "/home/u/.local/share/flatpak/exports/share:/usr/local/share:/usr/share";
        let appdir = "/tmp/.mount_Handy_DGEpEI";
        let polluted = format!("{appdir}/usr/share/:{appdir}/usr/share:/usr/share:{session}");
        assert_eq!(session_data_dirs(&polluted, appdir), session);
        assert_eq!(
            bus_addr_path("unix:abstract=/tmp/dbus-Xy,guid=1;unix:path=/x"),
            Some(("/tmp/dbus-Xy".into(), true))
        );
    }

    /// The keymap the virtual-keyboard backend uploads has to compile, with
    /// every slot carrying its keysym, or the compositor drops us.
    #[test]
    fn generated_keymap_compiles() {
        let slots: Vec<u32> = "a€\n→日".chars().filter_map(char_to_keysym).collect();
        let text = keymap_text(&slots).trim_end_matches('\0').to_string();
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_string(
            &ctx,
            text,
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .expect("the generated keymap compiles");
        for (i, &ks) in slots.iter().enumerate() {
            let syms = keymap.key_get_syms_by_level(xkb::Keycode::new(i as u32 + 9), 0, 0);
            assert_eq!(syms, [xkb::Keysym::new(ks)]);
        }
    }

    /// Talks to whatever session runs the tests without typing into it:
    /// connects, reads the layout and looks 'a' up. Skips where there is no
    /// display, as in CI.
    #[test]
    fn reads_the_layout_of_the_running_session() {
        let a = char_to_keysym('a').unwrap();
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            let mut wl = Wayland::connect().unwrap();
            wl.get_globals().unwrap();
            let keymap = fetch_seat_keymap(&mut wl).unwrap();
            assert!(keymap.find_key(a).is_some(), "'a' is on the layout");
            Bus::connect().unwrap(); // Hello and back, if there is a bus
        }
        if let Ok(disp) = std::env::var("DISPLAY") {
            let x = X11::connect(&disp, Slots::default()).unwrap();
            assert!(x.layout_kc(a).is_some(), "'a' is on the layout");
        }
    }
}
