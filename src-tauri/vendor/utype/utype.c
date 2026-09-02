/* SPDX-License-Identifier: MIT */
/*
 * libutype - fake keyboard input on Linux, with no dependencies but libc.
 *
 * It speaks the Wayland and X11 protocols directly over their display sockets
 * (so neither libwayland nor libX11 is linked) and loads libxkbcommon at
 * runtime with dlopen (so no -dev headers are needed). Five backends are
 * chosen automatically:
 *
 *   - zwp_virtual_keyboard_manager_v1, the protocol wtype uses, on wlroots
 *     compositors (Sway, Hyprland, ...). It uploads its own keymap and presses
 *     synthetic keys, so it types any character regardless of the layout.
 *
 *   - org_kde_kwin_fake_input, KWin's privileged protocol, on KDE Plasma, where
 *     the first backend is not offered. It presses the physical keys that
 *     produce each character on the current layout, so it can only type what
 *     that layout produces; anything else is skipped with a warning. KWin only
 *     exposes fake_input to executables that request it in an installed
 *     .desktop file, so one is installed for the calling binary on first run.
 *
 *   - mutter's private remote-desktop D-Bus API, on the desktops built from
 *     mutter that offer neither Wayland protocol: GNOME, and the forks behind
 *     Cinnamon, Budgie and Pantheon. It takes the same evdev key events as
 *     fake_input and translates them through the session keymap the same way,
 *     so it shares both that backend's typing loop and its limitation. D-Bus
 *     is spoken directly over the bus socket.
 *
 *   - ydotoold, the last resort on a Wayland compositor that offers none of
 *     the above. The daemon owns a uinput device and replays whatever we hand
 *     it into the kernel, so it reaches anything that reads a keyboard, but it
 *     has to be running and its socket has to be writable.
 *
 *   - XTEST, on a plain X11 session (no WAYLAND_DISPLAY, but DISPLAY set).
 *     XTEST fakes keycodes rather than keysyms, so keys the layout already
 *     provides are pressed where they sit, with Shift where the layout wants
 *     it. Only what the layout cannot produce at all is parked on a keycode it
 *     leaves empty, which is what keeps the backend able to type anything; the
 *     mapping is restored before exit. Plain text costs no remapping.
 *
 * Wayland wins when both are available: on a Wayland session with Xwayland
 * both variables are set, but only the native protocols reach every window.
 * Setting UTYPE_PROTOCOL pins the choice instead, and utype_verbose() narrates
 * the whole decision on stderr.
 *
 * The public API is in utype.h.
 */
#define _GNU_SOURCE
#include "utype.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <stddef.h>
#include <string.h>
#include <strings.h>
#include <errno.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <dlfcn.h>
#include <time.h>
#include <poll.h>
#include <netdb.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/mman.h>
#include <sys/stat.h>

/* evdev key codes, used by the fake_input backend to press physical keys */
enum { KEY_ESC = 1, KEY_BACKSPACE = 14, KEY_TAB = 15, KEY_ENTER = 28,
       KEY_LEFTCTRL = 29, KEY_LEFTSHIFT = 42, KEY_LEFTALT = 56,
       KEY_CAPSLOCK = 58, KEY_RIGHTALT = 100, KEY_LEFTMETA = 125 };

/* a few keysyms we need by value */
#define KS_RETURN 0xff0d
#define KS_TAB    0xff09
#define KS_ESCAPE 0xff1b

#define HOLD_MS 2   /* how long each key is held */

/* what to say when the session turns out to speak none of the protocols */
#define NO_INPUT_PROTOCOL \
    "no supported input protocol.\n" \
    "       This tool needs a compositor offering the virtual-keyboard\n" \
    "       protocol (wlroots: Sway, Hyprland, ...), KWin (KDE Plasma), or a\n" \
    "       mutter-style remote-desktop D-Bus API (GNOME, Cinnamon, Budgie).\n" \
    "       Failing all of those, start ydotoold and try again."


/* the actions to perform, set by utype_run() */
static const struct utype_cmd *cmds;
static int ncmds;

static void die(const char *fmt, ...) {
    va_list ap; va_start(ap, fmt);
    fprintf(stderr, "utype: "); vfprintf(stderr, fmt, ap);
    va_end(ap); fputc('\n', stderr); exit(1);
}
static void warn(const char *fmt, ...) {
    va_list ap; va_start(ap, fmt);
    fprintf(stderr, "utype: "); vfprintf(stderr, fmt, ap);
    va_end(ap); fputc('\n', stderr);
}
static int verbose;

void utype_verbose(int on) { verbose = on; }

static void vlog(const char *fmt, ...) {
    if (!verbose) return;
    va_list ap; va_start(ap, fmt);
    fprintf(stderr, "utype: "); vfprintf(stderr, fmt, ap);
    va_end(ap); fputc('\n', stderr);
}
static const char *yesno(int v) { return v ? "yes" : "no"; }

static void nap(long ms) {
    if (ms <= 0) return;
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&t, NULL);
}

/* ------------------------------------------------------------------ Wayland */

static int sock = -1;
static uint32_t next_id;
static uint32_t registry_id, sync_id, keyboard_id;
static uint32_t fake_name, fake_ver, seat_name, seat_ver, vkm_name, vkm_ver;
static uint32_t fake_id, seat_id, vkm_id, vk_id;
static int have_fake, have_seat, have_vkm, sync_done;
static int keymap_fd;
static uint32_t keymap_size;

static unsigned char inbuf[1 << 16];
static size_t inlen;
static int fdq[8], fdq_n;

static uint32_t alloc_id(void) { return next_id++; }

static void put_u32(unsigned char *b, size_t *off, uint32_t v) {
    memcpy(b + *off, &v, 4); *off += 4;
}
static void put_str(unsigned char *b, size_t *off, const char *s) {
    uint32_t len = (uint32_t)strlen(s) + 1;
    put_u32(b, off, len);
    memcpy(b + *off, s, len); *off += len;
    while (*off & 3) b[(*off)++] = 0;
}

static void write_all(const unsigned char *buf, size_t size) {
    size_t off = 0;
    while (off < size) {
        ssize_t n = write(sock, buf + off, size - off);
        if (n < 0) { if (errno == EINTR) continue; die("write to compositor failed"); }
        off += (size_t)n;
    }
}

static void send_msg(uint32_t obj, uint16_t opcode,
                     const unsigned char *body, uint16_t blen) {
    unsigned char buf[8 + 256];
    uint16_t size = (uint16_t)(8 + blen);
    if (size > sizeof buf) die("outgoing message too large");
    memcpy(buf, &obj, 4);
    uint32_t w1 = ((uint32_t)size << 16) | opcode;
    memcpy(buf + 4, &w1, 4);
    memcpy(buf + 8, body, blen);
    write_all(buf, size);
}

/* like send_msg, but pass one file descriptor in the ancillary data */
static void send_msg_fd(uint32_t obj, uint16_t opcode,
                        const unsigned char *body, uint16_t blen, int fd) {
    unsigned char buf[8 + 256];
    uint16_t size = (uint16_t)(8 + blen);
    if (size > sizeof buf) die("outgoing message too large");
    memcpy(buf, &obj, 4);
    uint32_t w1 = ((uint32_t)size << 16) | opcode;
    memcpy(buf + 4, &w1, 4);
    memcpy(buf + 8, body, blen);

    struct iovec iov = { buf, size };
    union { char b[CMSG_SPACE(sizeof(int))]; struct cmsghdr align; } cm;
    struct msghdr mh = { 0 };
    mh.msg_iov = &iov; mh.msg_iovlen = 1;
    mh.msg_control = cm.b; mh.msg_controllen = sizeof cm.b;
    struct cmsghdr *c = CMSG_FIRSTHDR(&mh);
    c->cmsg_level = SOL_SOCKET; c->cmsg_type = SCM_RIGHTS;
    c->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(c), &fd, sizeof(int));
    for (;;) {
        ssize_t n = sendmsg(sock, &mh, 0);
        if (n < 0) { if (errno == EINTR) continue; die("sendmsg to compositor failed"); }
        break;   /* the message is tiny; a short send will not happen */
    }
}

static void recv_some(void) {
    unsigned char tmp[4096];
    union { char b[CMSG_SPACE(sizeof(int) * 8)]; struct cmsghdr align; } cmsg;
    struct iovec iov = { tmp, sizeof tmp };
    struct msghdr mh = { 0 };
    mh.msg_iov = &iov; mh.msg_iovlen = 1;
    mh.msg_control = cmsg.b; mh.msg_controllen = sizeof cmsg.b;
    ssize_t n = recvmsg(sock, &mh, 0);
    if (n < 0) { if (errno == EINTR || errno == EAGAIN) return; die("read from compositor failed"); }
    if (n == 0) die("compositor closed the connection");
    for (struct cmsghdr *c = CMSG_FIRSTHDR(&mh); c; c = CMSG_NXTHDR(&mh, c))
        if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) {
            int cnt = (int)((c->cmsg_len - CMSG_LEN(0)) / sizeof(int));
            int *fds = (int *)CMSG_DATA(c);
            for (int i = 0; i < cnt && fdq_n < 8; i++) fdq[fdq_n++] = fds[i];
        }
    if (inlen + (size_t)n > sizeof inbuf) die("incoming buffer overflow");
    memcpy(inbuf + inlen, tmp, (size_t)n); inlen += (size_t)n;
}

static void dispatch_one(uint32_t obj, uint16_t op, unsigned char *body, uint16_t blen) {
    if (obj == 1) {                      /* wl_display */
        if (op == 0) {                   /* error(object, code, message) */
            uint32_t code, mlen;
            memcpy(&code, body + 4, 4);
            memcpy(&mlen, body + 8, 4);
            fprintf(stderr, "utype: wayland error %u: %.*s\n",
                    code, mlen ? (int)mlen - 1 : 0, body + 12);
            exit(1);
        }
        return;                          /* delete_id: ignore */
    }
    if (obj == registry_id && op == 0) { /* global(name, interface, version) */
        uint32_t name, ilen, ver;
        memcpy(&name, body, 4);
        memcpy(&ilen, body + 4, 4);
        const char *iface = (const char *)(body + 8);
        size_t pad = (ilen + 3u) & ~3u;
        memcpy(&ver, body + 8 + pad, 4);
        if (!strcmp(iface, "org_kde_kwin_fake_input"))              { have_fake = 1; fake_name = name; fake_ver = ver; }
        else if (!strcmp(iface, "zwp_virtual_keyboard_manager_v1")) { have_vkm = 1; vkm_name = name; vkm_ver = ver; }
        else if (!strcmp(iface, "wl_seat"))                         { have_seat = 1; seat_name = name; seat_ver = ver; }
        return;
    }
    if (obj == sync_id && op == 0) { sync_done = 1; return; }
    if (obj == keyboard_id && op == 0) { /* keymap(format, fd, size) */
        memcpy(&keymap_size, body + 4, 4);
        if (fdq_n > 0) { keymap_fd = fdq[0]; memmove(fdq, fdq + 1, (size_t)(--fdq_n) * sizeof(int)); }
        return;
    }
    (void)blen;
}

static void dispatch_pending(void) {
    size_t off = 0;
    while (inlen - off >= 8) {
        uint32_t obj, w1;
        memcpy(&obj, inbuf + off, 4);
        memcpy(&w1, inbuf + off + 4, 4);
        uint16_t size = (uint16_t)(w1 >> 16), op = (uint16_t)(w1 & 0xffff);
        if (size < 8) die("malformed message");
        if (inlen - off < size) break;
        dispatch_one(obj, op, inbuf + off + 8, (uint16_t)(size - 8));
        off += size;
    }
    if (off) { memmove(inbuf, inbuf + off, inlen - off); inlen -= off; }
}

/* flush, then block until the compositor has processed everything so far */
static void roundtrip(void) {
    sync_id = alloc_id(); sync_done = 0;
    unsigned char b[8]; size_t o = 0; put_u32(b, &o, sync_id);
    send_msg(1, 0, b, (uint16_t)o);      /* wl_display.sync */
    while (!sync_done) {
        dispatch_pending();
        if (sync_done) break;
        struct pollfd pfd = { sock, POLLIN, 0 };
        if (poll(&pfd, 1, 3000) <= 0) die("timed out waiting for the compositor");
        recv_some();
    }
}

/* Connect to the Wayland display. Returns 0 instead of dying when the caller
 * has somewhere else to go (an X11 session) and `fatal` is 0. */
static int wl_connect(int fatal) {
    const char *disp = getenv("WAYLAND_DISPLAY");
    if (!disp) disp = "wayland-0";
    char path[512];
    if (disp[0] == '/') snprintf(path, sizeof path, "%s", disp);
    else {
        const char *rt = getenv("XDG_RUNTIME_DIR");
        if (!rt) {
            if (fatal) die("XDG_RUNTIME_DIR is not set");
            return 0;
        }
        snprintf(path, sizeof path, "%s/%s", rt, disp);
    }
    struct sockaddr_un a = { 0 };
    a.sun_family = AF_UNIX;
    if (strlen(path) >= sizeof a.sun_path) die("Wayland socket path is too long");
    memcpy(a.sun_path, path, strlen(path) + 1);
    sock = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (sock < 0) die("socket() failed");
    if (connect(sock, (struct sockaddr *)&a, sizeof a) < 0) {
        close(sock); sock = -1;
        if (fatal) die("cannot connect to the Wayland display");
        return 0;
    }
    next_id = 2; inlen = 0; fdq_n = 0;
    have_fake = have_seat = have_vkm = 0; keymap_fd = -1;
    vlog("connected to the Wayland display at %s", path);
    return 1;
}

static void wl_close(void) { if (sock >= 0) close(sock); sock = -1; }

static void get_globals(void) {
    registry_id = alloc_id();
    unsigned char b[8]; size_t o = 0; put_u32(b, &o, registry_id);
    send_msg(1, 1, b, (uint16_t)o);      /* wl_display.get_registry */
    roundtrip();
}

static uint32_t bind_global(uint32_t name, const char *iface, uint32_t version) {
    uint32_t id = alloc_id();
    unsigned char b[128]; size_t o = 0;
    put_u32(b, &o, name);
    put_str(b, &o, iface);
    put_u32(b, &o, version);
    put_u32(b, &o, id);
    send_msg(registry_id, 0, b, (uint16_t)o);   /* wl_registry.bind */
    return id;
}

/* -------------------------------------------------------------- xkbcommon */

static uint32_t (*xkb_keysym_from_name)(const char *, int);
static uint32_t (*xkb_utf32_to_keysym)(uint32_t);
static int      (*xkb_keysym_get_name)(uint32_t, char *, size_t);
static void   *(*xkb_context_new)(int);
static void   *(*xkb_keymap_new_from_string)(void *, const char *, int, int);
static uint32_t (*xkb_keymap_min_keycode)(void *);
static uint32_t (*xkb_keymap_max_keycode)(void *);
static uint32_t (*xkb_keymap_num_levels_for_key)(void *, uint32_t, uint32_t);
static int      (*xkb_keymap_key_get_syms_by_level)(void *, uint32_t, uint32_t, uint32_t, const uint32_t **);
static void    *xkb_lib, *xkb_keymap;

#define SYM(n) do { *(void **)(&n) = dlsym(xkb_lib, #n); if (!n) die("libxkbcommon missing " #n); } while (0)

/* symbols needed to turn characters and key names into keysyms (idempotent) */
static void load_xkb_base(void) {
    if (xkb_lib) return;
    xkb_lib = dlopen("libxkbcommon.so.0", RTLD_NOW | RTLD_LOCAL);
    if (!xkb_lib) die("cannot load libxkbcommon.so.0");
    SYM(xkb_keysym_from_name);
    SYM(xkb_utf32_to_keysym);
    SYM(xkb_keysym_get_name);
}

/* symbols and state needed to look keysyms up in the compositor's own keymap */
static void load_xkb_keymap(void) {
    SYM(xkb_context_new);
    SYM(xkb_keymap_new_from_string);
    SYM(xkb_keymap_min_keycode);
    SYM(xkb_keymap_max_keycode);
    SYM(xkb_keymap_num_levels_for_key);
    SYM(xkb_keymap_key_get_syms_by_level);
    void *ctx = xkb_context_new(0);
    if (!ctx) die("xkb_context_new failed");
    char *map = mmap(NULL, keymap_size, PROT_READ, MAP_PRIVATE, keymap_fd, 0);
    if (map == MAP_FAILED) die("mmap of the compositor keymap failed");
    xkb_keymap = xkb_keymap_new_from_string(ctx, map, 1 /* TEXT_V1 */, 0);
    munmap(map, keymap_size);
    close(keymap_fd);
    if (!xkb_keymap) die("could not compile the compositor keymap");
}

/* Find a key + shift level that produces keysym on the active layout (group 0).
 * Returns 1 and fills *code (evdev) and *level, or 0 if not reachable. */
static int find_key(uint32_t keysym, uint32_t *code, uint32_t *level) {
    uint32_t lo = xkb_keymap_min_keycode(xkb_keymap);
    uint32_t hi = xkb_keymap_max_keycode(xkb_keymap);
    for (uint32_t kc = lo; kc <= hi; kc++) {
        uint32_t levels = xkb_keymap_num_levels_for_key(xkb_keymap, kc, 0);
        for (uint32_t lv = 0; lv < levels; lv++) {
            const uint32_t *syms;
            int n = xkb_keymap_key_get_syms_by_level(xkb_keymap, kc, 0, lv, &syms);
            for (int i = 0; i < n; i++)
                if (syms[i] == keysym) { *code = kc - 8; *level = lv; return 1; }
        }
    }
    return 0;
}

/* map a character to the keysym we should type for it */
static uint32_t char_to_keysym(uint32_t cp) {
    if (cp == '\n') return KS_RETURN;
    if (cp == '\t') return KS_TAB;
    if (cp == 0x1b) return KS_ESCAPE;
    return xkb_utf32_to_keysym(cp);
}

/* decode one UTF-8 character; returns bytes consumed, 0 at end of string */
static int utf8_next(const unsigned char *s, uint32_t *cp) {
    unsigned char c = s[0];
    if (!c) return 0;
    if (c < 0x80) { *cp = c; return 1; }
    if ((c >> 5) == 0x6 && (s[1] & 0xc0) == 0x80) {
        *cp = ((uint32_t)(c & 0x1f) << 6) | (s[1] & 0x3f); return 2;
    }
    if ((c >> 4) == 0xe && (s[1] & 0xc0) == 0x80 && (s[2] & 0xc0) == 0x80) {
        *cp = ((uint32_t)(c & 0x0f) << 12) | ((uint32_t)(s[1] & 0x3f) << 6) | (s[2] & 0x3f); return 3;
    }
    if ((c >> 3) == 0x1e && (s[1] & 0xc0) == 0x80 && (s[2] & 0xc0) == 0x80 && (s[3] & 0xc0) == 0x80) {
        *cp = ((uint32_t)(c & 0x07) << 18) | ((uint32_t)(s[1] & 0x3f) << 12)
            | ((uint32_t)(s[2] & 0x3f) << 6) | (s[3] & 0x3f); return 4;
    }
    *cp = c; return 1;   /* invalid byte: skip it */
}

static const char *mod_name(int mod) {
    switch (mod) {
    case UTYPE_SHIFT: return "shift";
    case UTYPE_CAPS:  return "capslock";
    case UTYPE_CTRL:  return "ctrl";
    case UTYPE_ALT:   return "alt";
    case UTYPE_LOGO:  return "logo";
    case UTYPE_ALTGR: return "altgr";
    }
    return "?";
}

static int utf8_count(const char *t) {
    const unsigned char *p = (const unsigned char *)t;
    uint32_t cp;
    int n = 0;
    for (int adv; (adv = utf8_next(p, &cp)); p += adv) n++;
    return n;
}

/* One line per action. The text itself is never logged, only how much of it
 * there is: people type passwords with this. */
static void vlog_cmd(const struct utype_cmd *c) {
    if (!verbose) return;
    char nm[256];
    switch (c->type) {
    case UTYPE_TEXT:
        vlog("typing %d characters", c->text ? utf8_count(c->text) : 0);
        break;
    case UTYPE_TAP: case UTYPE_PRESS: case UTYPE_RELEASE:
        if (xkb_keysym_get_name(c->keysym, nm, sizeof nm) <= 0) strcpy(nm, "?");
        vlog("%s %s", c->type == UTYPE_TAP ? "tapping" :
                      c->type == UTYPE_PRESS ? "holding" : "releasing", nm);
        break;
    case UTYPE_MODPRESS: case UTYPE_MODRELEASE:
        vlog("%s the %s modifier",
             c->type == UTYPE_MODPRESS ? "holding" : "releasing", mod_name(c->mod));
        break;
    case UTYPE_SLEEP:
        vlog("sleeping %d ms", c->ms);
        break;
    }
}

/* ------------------------------------------- backend A: virtual keyboard */

static uint32_t *slots; static int nslots;

static int slot_for(uint32_t ks) {
    for (int i = 0; i < nslots; i++) if (slots[i] == ks) return i + 1;
    slots = realloc(slots, (nslots + 1) * sizeof *slots);
    slots[nslots++] = ks;
    return nslots;
}

/* first pass: give every keysym we will type a slot in the keymap */
static void collect_slots(void) {
    for (int i = 0; i < ncmds; i++) {
        const struct utype_cmd *c = &cmds[i];
        if (c->type == UTYPE_TAP || c->type == UTYPE_PRESS || c->type == UTYPE_RELEASE) {
            slot_for(c->keysym);
        } else if (c->type == UTYPE_TEXT && c->text) {
            const unsigned char *p = (const unsigned char *)c->text;
            uint32_t cp;
            for (int adv; (adv = utf8_next(p, &cp)); p += adv) {
                if (cp == '\r') continue;
                uint32_t ks = char_to_keysym(cp);
                char nm[256];
                if (!ks || xkb_keysym_get_name(ks, nm, sizeof nm) <= 0) continue;
                slot_for(ks);
            }
        }
    }
}

/* build the keymap text and hand it to the compositor */
struct sb { char *p; size_t len, cap; };
static void sb_addf(struct sb *s, const char *fmt, ...) {
    va_list ap, ap2;
    va_start(ap, fmt); va_copy(ap2, ap);
    int need = vsnprintf(NULL, 0, fmt, ap); va_end(ap);
    if (need < 0) die("vsnprintf failed");
    if (s->len + need + 1 > s->cap) { s->cap = (s->len + need + 1) * 2; s->p = realloc(s->p, s->cap); }
    vsnprintf(s->p + s->len, need + 1, fmt, ap2); va_end(ap2);
    s->len += need;
}

static void upload_keymap(void) {
    struct sb s = { 0 };
    sb_addf(&s, "xkb_keymap {\n");
    sb_addf(&s, "xkb_keycodes \"(unnamed)\" {\nminimum = 8;\nmaximum = %d;\n", nslots + 8 + 1);
    for (int i = 0; i < nslots; i++) sb_addf(&s, "<K%d> = %d;\n", i + 1, i + 8 + 1);
    sb_addf(&s, "};\n");
    sb_addf(&s, "xkb_types \"(unnamed)\" { include \"complete\" };\n");
    sb_addf(&s, "xkb_compatibility \"(unnamed)\" { include \"complete\" };\n");
    sb_addf(&s, "xkb_symbols \"(unnamed)\" {\n");
    for (int i = 0; i < nslots; i++) {
        char nm[256];
        if (xkb_keysym_get_name(slots[i], nm, sizeof nm) <= 0) strcpy(nm, "NoSymbol");
        sb_addf(&s, "key <K%d> {[%s]};\n", i + 1, nm);
    }
    sb_addf(&s, "};\n};\n");

    int fd = memfd_create("utype-keymap", MFD_CLOEXEC);
    if (fd < 0) die("memfd_create failed");
    size_t off = 0, total = s.len + 1;   /* include the trailing NUL */
    s.p[s.len] = 0;
    while (off < total) {
        ssize_t n = write(fd, s.p + off, total - off);
        if (n < 0) { if (errno == EINTR) continue; die("writing the keymap failed"); }
        off += (size_t)n;
    }
    unsigned char b[16]; size_t o = 0;
    put_u32(b, &o, 1);                /* format XKB_V1 */
    put_u32(b, &o, (uint32_t)total);  /* size */
    send_msg_fd(vk_id, 0, b, (uint16_t)o, fd);   /* virtual_keyboard.keymap */
    roundtrip();
    vlog("uploaded a keymap carrying %d keysyms", nslots);
    close(fd);
    free(s.p);
}

static uint32_t vkbd_mod;

static void vk_key(uint32_t slot, int state) {
    unsigned char b[16]; size_t o = 0;
    put_u32(b, &o, 0); put_u32(b, &o, slot); put_u32(b, &o, (uint32_t)state);
    send_msg(vk_id, 1, b, (uint16_t)o);          /* virtual_keyboard.key */
    roundtrip();
}
static void vk_tap(uint32_t slot, int delay) {
    vk_key(slot, 1); nap(HOLD_MS); vk_key(slot, 0); nap(HOLD_MS); nap(delay);
}
static void vk_send_mods(void) {
    unsigned char b[16]; size_t o = 0;
    put_u32(b, &o, vkbd_mod & ~UTYPE_CAPS);  /* depressed */
    put_u32(b, &o, 0);                        /* latched */
    put_u32(b, &o, vkbd_mod & UTYPE_CAPS);   /* locked */
    put_u32(b, &o, 0);                        /* group */
    send_msg(vk_id, 2, b, (uint16_t)o);          /* virtual_keyboard.modifiers */
    roundtrip();
}

static void run_virtual_keyboard(void) {
    vlog("using virtual-keyboard (zwp_virtual_keyboard_manager_v1, version %u)", vkm_ver);
    if (!have_seat) die("the compositor offers no wl_seat");
    seat_id = bind_global(seat_name, "wl_seat", seat_ver < 7 ? seat_ver : 7);
    vkm_id = bind_global(vkm_name, "zwp_virtual_keyboard_manager_v1", 1);
    vk_id = alloc_id();
    unsigned char b[16]; size_t o = 0;
    put_u32(b, &o, seat_id); put_u32(b, &o, vk_id);
    send_msg(vkm_id, 0, b, (uint16_t)o);   /* manager.create_virtual_keyboard */

    collect_slots();
    upload_keymap();

    for (int i = 0; i < ncmds; i++) {
        const struct utype_cmd *c = &cmds[i];
        vlog_cmd(c);
        switch (c->type) {
        case UTYPE_TEXT: {
            const unsigned char *p = (const unsigned char *)c->text;
            uint32_t cp;
            for (int adv; (adv = utf8_next(p, &cp)); p += adv) {
                if (cp == '\r') continue;
                uint32_t ks = char_to_keysym(cp);
                char nm[256];
                if (!ks || xkb_keysym_get_name(ks, nm, sizeof nm) <= 0) {
                    warn("U+%04X has no keysym; skipped", cp); continue;
                }
                vk_tap(slot_for(ks), c->ms);
            }
            break;
        }
        case UTYPE_TAP:     vk_tap(slot_for(c->keysym), c->ms); break;
        case UTYPE_PRESS:   vk_key(slot_for(c->keysym), 1); break;
        case UTYPE_RELEASE: vk_key(slot_for(c->keysym), 0); break;
        case UTYPE_MODPRESS:   vkbd_mod |= c->mod; vk_send_mods(); break;
        case UTYPE_MODRELEASE: vkbd_mod &= ~c->mod; vk_send_mods(); break;
        case UTYPE_SLEEP:   nap(c->ms); break;
        }
    }
    roundtrip();
}

/* ------------------------------------------ backends B and C: keycodes */
/*
 * KWin's fake_input and mutter's remote-desktop D-Bus API both take evdev
 * keycodes and run them through the session's own keymap, so everything from
 * here to the end of the section is shared: only how a single key event
 * reaches the session differs, and that is what emit_key points at.
 */

static void (*emit_key)(uint32_t code, uint32_t state);
static void (*emit_done)(void);      /* wait for the last events to land */

static void fi_key_ev(uint32_t code, uint32_t state) {
    unsigned char b[8]; size_t o = 0;
    put_u32(b, &o, code); put_u32(b, &o, state);
    send_msg(fake_id, 10, b, (uint16_t)o);       /* fake_input.keyboard_key */
}
static void tap(uint32_t code) { emit_key(code, 1); nap(HOLD_MS); emit_key(code, 0); }

static uint32_t mod_phys(int mod) {
    switch (mod) {
    case UTYPE_SHIFT: return KEY_LEFTSHIFT;
    case UTYPE_CTRL:  return KEY_LEFTCTRL;
    case UTYPE_ALT:   return KEY_LEFTALT;
    case UTYPE_LOGO:  return KEY_LEFTMETA;
    case UTYPE_ALTGR: return KEY_RIGHTALT;
    default:           return 0;
    }
}

static int held_mods;   /* modifiers held down via UTYPE_MODPRESS */

/* press a keysym on the real layout, adding Shift/AltGr only if not held */
static void b_stroke(uint32_t ks, int delay) {
    uint32_t code, level;
    if (!find_key(ks, &code, &level) || level > 3) {
        char nm[256];
        if (xkb_keysym_get_name(ks, nm, sizeof nm) <= 0) strcpy(nm, "?");
        warn("'%s' is not on the active keyboard layout; skipped", nm);
        return;
    }
    int ts = (level == 1 || level == 3) && !(held_mods & UTYPE_SHIFT);
    int ta = (level == 2 || level == 3) && !(held_mods & UTYPE_ALTGR);
    if (ts) emit_key(KEY_LEFTSHIFT, 1);
    if (ta) emit_key(KEY_RIGHTALT, 1);
    tap(code);
    if (ta) emit_key(KEY_RIGHTALT, 0);
    if (ts) emit_key(KEY_LEFTSHIFT, 0);
    nap(delay);
}

static int caps_lock_on(void) {
    DIR *d = opendir("/sys/class/leds");
    if (!d) return 0;
    int on = 0;
    struct dirent *e;
    while ((e = readdir(d)))
        if (strstr(e->d_name, "capslock")) {
            char p[512];
            snprintf(p, sizeof p, "/sys/class/leds/%s/brightness", e->d_name);
            FILE *f = fopen(p, "r");
            if (f) { int ch = fgetc(f); if (ch >= '1' && ch <= '9') on = 1; fclose(f); }
        }
    closedir(d);
    return on;
}

static void run_keycode_commands(void) {
    int caps = caps_lock_on();           /* caps lock would invert letter case */
    if (caps) { tap(KEY_CAPSLOCK); nap(20); }

    for (int i = 0; i < ncmds; i++) {
        const struct utype_cmd *c = &cmds[i];
        vlog_cmd(c);
        switch (c->type) {
        case UTYPE_TEXT: {
            const unsigned char *p = (const unsigned char *)c->text;
            uint32_t cp;
            for (int adv; (adv = utf8_next(p, &cp)); p += adv) {
                if (cp == '\r') continue;
                uint32_t ks = char_to_keysym(cp);
                if (!ks) { warn("U+%04X has no keysym; skipped", cp); continue; }
                b_stroke(ks, c->ms);
            }
            break;
        }
        case UTYPE_TAP: b_stroke(c->keysym, c->ms); break;
        case UTYPE_PRESS: case UTYPE_RELEASE: {
            uint32_t code, level;
            if (find_key(c->keysym, &code, &level)) emit_key(code, c->type == UTYPE_PRESS);
            else warn("this key is not on the active layout; skipped");
            break;
        }
        case UTYPE_MODPRESS:
            if (c->mod == UTYPE_CAPS) tap(KEY_CAPSLOCK); else emit_key(mod_phys(c->mod), 1);
            held_mods |= c->mod; break;
        case UTYPE_MODRELEASE:
            if (c->mod == UTYPE_CAPS) tap(KEY_CAPSLOCK); else emit_key(mod_phys(c->mod), 0);
            held_mods &= ~c->mod; break;
        case UTYPE_SLEEP: nap(c->ms); break;
        }
    }

    if (caps) tap(KEY_CAPSLOCK);
    emit_done();                          /* make sure the keys land before we exit */
}

/* Read the compositor's keymap off the seat, which is how both keycode
 * backends learn where the current layout puts each character. */
static void fetch_seat_keymap(void) {
    if (!have_seat) die("the compositor offers no wl_seat; cannot read the keymap");
    seat_id = bind_global(seat_name, "wl_seat", seat_ver < 5 ? seat_ver : 5);
    keyboard_id = alloc_id();
    unsigned char b[16]; size_t o = 0;
    put_u32(b, &o, keyboard_id);
    send_msg(seat_id, 1, b, (uint16_t)o); /* wl_seat.get_keyboard */
    roundtrip();
    if (keymap_fd < 0) die("did not receive a keymap from the seat");
    vlog("read the session keymap off wl_seat (%u bytes)", keymap_size);
    load_xkb_keymap();
}

/* -------------------------------------- backend B: fake_input, on KWin */

static int is_kde(void) {
    const char *d = getenv("XDG_CURRENT_DESKTOP");
    if (d && strcasestr(d, "KDE")) return 1;
    d = getenv("XDG_SESSION_DESKTOP");
    return d && strcasestr(d, "KDE");
}

static void install_desktop(void) {
    char exe[512];
    ssize_t r = readlink("/proc/self/exe", exe, sizeof exe - 1);
    if (r < 0) die("readlink /proc/self/exe failed");
    exe[r] = 0;

    char dp[600];
    const char *xdh = getenv("XDG_DATA_HOME");
    if (xdh && xdh[0]) snprintf(dp, sizeof dp, "%s/applications/utype.desktop", xdh);
    else snprintf(dp, sizeof dp, "%s/.local/share/applications/utype.desktop", getenv("HOME"));
    for (char *p = dp + 1; *p; p++)      /* mkdir -p the parent dirs */
        if (*p == '/') { *p = 0; mkdir(dp, 0755); *p = '/'; }

    FILE *f = fopen(dp, "w");
    if (!f) die("cannot write the authorization .desktop file");
    fprintf(f,
        "[Desktop Entry]\n"
        "Type=Application\n"
        "Name=utype\n"
        "NoDisplay=true\n"
        "Terminal=false\n"
        "Exec=%s\n"
        "X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input\n", exe);
    fclose(f);

    if (system("kbuildsycoca6 --noincremental >/dev/null 2>&1 || "
               "kbuildsycoca5 --noincremental >/dev/null 2>&1") == -1)
        die("could not run kbuildsycoca");
}

/* Make sure fake_input is available, self-installing the .desktop file if not. */
static void ensure_fake_input(void) {
    if (have_fake) return;
    vlog("fake_input is not offered yet; installing the .desktop that asks for it");
    wl_close();
    install_desktop();
    for (int i = 0; i < 16; i++) {        /* KWin reloads its service cache async */
        nap(500);
        wl_connect(1);
        get_globals();
        if (have_fake) { vlog("fake_input appeared after %d ms", (i + 1) * 500); return; }
        wl_close();
    }
    die("org_kde_kwin_fake_input is still unavailable after self-install.\n"
        "       A re-login may be needed for KWin to pick up the change.");
}

static void run_fake_input(void) {
    ensure_fake_input();
    vlog("using fake-input (org_kde_kwin_fake_input, version %u)", fake_ver);
    if (fake_ver < 4) die("this KWin's fake_input is too old (need version 4+)");
    fake_id = bind_global(fake_name, "org_kde_kwin_fake_input", fake_ver < 6 ? fake_ver : 6);
    unsigned char b[128]; size_t o = 0;
    put_str(b, &o, "utype"); put_str(b, &o, "virtual keyboard input");
    send_msg(fake_id, 0, b, (uint16_t)o); /* fake_input.authenticate */

    fetch_seat_keymap();
    emit_key = fi_key_ev;
    emit_done = roundtrip;
    run_keycode_commands();
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

/* header field codes, from the D-Bus specification */
enum { D_PATH = 1, D_IFACE = 2, D_MEMBER = 3, D_ERRNAME = 4, D_REPLYSERIAL = 5,
       D_DEST = 6, D_SIGNATURE = 8 };
/* message types */
enum { D_CALL = 1, D_RETURN = 2, D_ERROR = 3 };

static int bus = -1;
static uint32_t bus_serial;
static int bus_swap;               /* the peer marshals in the other byte order */
static char bus_err[320];          /* what the last failed call came back with */

/* The names to try, in order. Upstream uses the service name as the manager
 * interface too, and the session interface is that plus ".Session"; the forks
 * follow suit, so two strings describe a flavor completely. */
static const struct rd_flavor {
    const char *service, *path;
} rd_flavors[] = {
    { "org.gnome.Mutter.RemoteDesktop",    "/org/gnome/Mutter/RemoteDesktop" },
    { "org.cinnamon.Muffin.RemoteDesktop", "/org/cinnamon/Muffin/RemoteDesktop" },
};
static const struct rd_flavor *rd;   /* whichever of them answered */
static char rd_path[256];            /* object path of the session we created */
static char rd_iface[96];            /* that session's interface name */

static int host_le(void) { uint16_t one = 1; return *(unsigned char *)&one; }

/* A message under construction. Everything we send is small and fixed-shape,
 * so one flat buffer with an offset is enough. */
struct dbus_msg { unsigned char b[512]; size_t n, body; };

static void d_room(struct dbus_msg *m, size_t k) {
    if (m->n + k > sizeof m->b) die("D-Bus message too large");
}
static void d_pad(struct dbus_msg *m, size_t a) {
    while (m->n & (a - 1)) { d_room(m, 1); m->b[m->n++] = 0; }
}
static void d_u32(struct dbus_msg *m, uint32_t v) {
    d_pad(m, 4); d_room(m, 4); memcpy(m->b + m->n, &v, 4); m->n += 4;
}
static void d_str(struct dbus_msg *m, const char *s) {    /* STRING, OBJECT_PATH */
    size_t l = strlen(s);
    d_u32(m, (uint32_t)l); d_room(m, l + 1);
    memcpy(m->b + m->n, s, l + 1); m->n += l + 1;
}
static void d_sig(struct dbus_msg *m, const char *s) {    /* SIGNATURE */
    size_t l = strlen(s);
    d_room(m, l + 2); m->b[m->n++] = (unsigned char)l;
    memcpy(m->b + m->n, s, l + 1); m->n += l + 1;
}
/* one header field: a struct, so 8-aligned, of a field code and a variant */
static void d_field(struct dbus_msg *m, uint8_t code, const char *type, const char *val) {
    d_pad(m, 8); d_room(m, 1); m->b[m->n++] = code;
    d_sig(m, type);
    if (type[0] == 'g') d_sig(m, val); else d_str(m, val);
}

/* Open a method call. Arguments matching `sig` are appended by the caller,
 * then d_send() fills in the lengths and writes the whole thing out. */
static void d_call(struct dbus_msg *m, const char *dest, const char *path,
                   const char *iface, const char *member, const char *sig) {
    memset(m, 0, sizeof *m);
    m->b[0] = host_le() ? 'l' : 'B';   /* we marshal in host order */
    m->b[1] = D_CALL;
    m->b[3] = 1;                       /* protocol version */
    m->n = 16;                         /* body length and serial are patched in later */
    d_field(m, D_PATH, "o", path);
    d_field(m, D_IFACE, "s", iface);
    d_field(m, D_MEMBER, "s", member);
    d_field(m, D_DEST, "s", dest);
    if (sig && *sig) d_field(m, D_SIGNATURE, "g", sig);
    uint32_t hlen = (uint32_t)(m->n - 16);
    memcpy(m->b + 12, &hlen, 4);
    d_pad(m, 8);                       /* the body starts on an 8-byte boundary */
    m->body = m->n;
}

static uint32_t d_send(struct dbus_msg *m) {
    uint32_t blen = (uint32_t)(m->n - m->body), serial = ++bus_serial;
    memcpy(m->b + 4, &blen, 4);
    memcpy(m->b + 8, &serial, 4);
    size_t off = 0;
    while (off < m->n) {
        ssize_t k = write(bus, m->b + off, m->n - off);
        if (k < 0) { if (errno == EINTR) continue; die("write to the session bus failed"); }
        off += (size_t)k;
    }
    return serial;
}

static void d_read(unsigned char *p, size_t n) {
    size_t off = 0;
    while (off < n) {
        ssize_t k = read(bus, p + off, n - off);
        if (k < 0) { if (errno == EINTR) continue; die("read from the session bus failed"); }
        if (k == 0) die("the session bus closed the connection");
        off += (size_t)k;
    }
}

static uint32_t d_get32(const unsigned char *p) {
    uint32_t v; memcpy(&v, p, 4);
    return bus_swap ? __builtin_bswap32(v) : v;
}

/* the string at *off, advancing past it; returns NULL if it runs off the end */
static const char *d_take_str(const unsigned char *buf, size_t len, size_t *off) {
    *off = (*off + 3) & ~(size_t)3;
    if (*off + 4 > len) return NULL;
    uint32_t l = d_get32(buf + *off); *off += 4;
    if (l >= len - *off) return NULL;          /* the NUL must fit too */
    const char *s = (const char *)buf + *off;
    *off += l + 1;
    return s;
}

/* Read messages until the reply to `serial` arrives; signals and anything else
 * on the bus are dropped. Returns 1 for a method return, filling `out` with
 * the reply's first argument (always a string in the calls we make), or 0 for
 * an error, whose name and message are left in bus_err. */
static int d_wait(uint32_t serial, char *out, size_t outsz) {
    for (;;) {
        unsigned char hdr[16];
        d_read(hdr, 16);
        bus_swap = (hdr[0] == 'l') != (host_le() != 0);
        if (hdr[3] != 1) die("the session bus speaks D-Bus protocol version %u", hdr[3]);
        uint32_t blen = d_get32(hdr + 4), rserial = 0, flen = d_get32(hdr + 12);
        size_t fpad = (8 - (flen & 7)) & 7;
        unsigned char *buf = malloc(flen + fpad + blen + 1);
        if (!buf) die("out of memory");
        d_read(buf, flen + fpad + blen);
        unsigned char *body = buf + flen + fpad;

        /* walk the header fields far enough to see which reply this is */
        char ename[192] = "";
        for (size_t o = 0; o < flen; ) {
            o = (o + 7) & ~(size_t)7;
            if (o + 4 > flen) break;
            uint8_t code = buf[o++];
            uint8_t slen = buf[o++];
            char t = (char)buf[o];
            o += (size_t)slen + 1;
            if (t == 's' || t == 'o') {
                const char *s = d_take_str(buf, flen, &o);
                if (!s) break;
                if (code == D_ERRNAME) snprintf(ename, sizeof ename, "%s", s);
            } else if (t == 'g') {
                if (o >= flen) break;
                o += (size_t)buf[o] + 2;
            } else if (t == 'u') {
                o = (o + 3) & ~(size_t)3;
                if (o + 4 > flen) break;
                uint32_t v = d_get32(buf + o); o += 4;
                if (code == D_REPLYSERIAL) rserial = v;
            } else break;              /* nothing else turns up in these headers */
        }

        if (rserial != serial) { free(buf); continue; }   /* not ours */

        size_t o = 0;
        const char *arg = blen ? d_take_str(body, blen, &o) : NULL;
        int ok = hdr[1] == D_RETURN;
        if (ok) { if (out && arg) snprintf(out, outsz, "%s", arg); }
        else snprintf(bus_err, sizeof bus_err, "%s%s%s", ename,
                      arg && *arg ? ": " : "", arg && *arg ? arg : "");
        free(buf);
        return ok;
    }
}

/* One address out of DBUS_SESSION_BUS_ADDRESS: a semicolon-separated list of
 * comma-separated key=value pairs, the first of which carries the transport. */
static int bus_addr_path(const char *addr, char *out, size_t outsz, int *abstract) {
    for (const char *p = addr; *p; ) {
        size_t n = strcspn(p, ",;");
        const char *eq = memchr(p, '=', n);
        if (eq) {
            const char *key = p;
            size_t klen = (size_t)(eq - p), vlen = n - klen - 1;
            if (klen > 5 && !memcmp(key, "unix:", 5)) { key += 5; klen -= 5; }
            if (vlen && ((klen == 4 && !memcmp(key, "path", 4)) ||
                         (klen == 8 && !memcmp(key, "abstract", 8)))) {
                if (vlen >= outsz) return 0;
                memcpy(out, eq + 1, vlen); out[vlen] = 0;
                *abstract = klen == 8;
                return 1;
            }
        }
        p += n;
        while (*p == ',' || *p == ';') p++;
    }
    return 0;
}

static int bus_line(char *out, size_t outsz) {
    size_t n = 0;
    for (;;) {
        char c;
        ssize_t k = read(bus, &c, 1);
        if (k < 0 && errno == EINTR) continue;
        if (k <= 0) return 0;
        if (c == '\n') { if (n && out[n - 1] == '\r') n--; out[n] = 0; return 1; }
        if (n + 1 < outsz) out[n++] = c;
    }
}

/* Connect to the session bus and get through the SASL handshake. Returns 0
 * when there is no bus to talk to, which is not by itself an error: the caller
 * has a better message to print than we do. */
static int bus_connect(void) {
    char path[256] = "";
    int abstract = 0;
    const char *addr = getenv("DBUS_SESSION_BUS_ADDRESS");
    if (!addr || !bus_addr_path(addr, path, sizeof path, &abstract)) {
        const char *rt = getenv("XDG_RUNTIME_DIR");
        if (!rt) return 0;
        snprintf(path, sizeof path, "%s/bus", rt);
    }
    size_t plen = strlen(path);
    struct sockaddr_un a = { 0 };
    a.sun_family = AF_UNIX;
    if (plen + abstract >= sizeof a.sun_path) return 0;
    memcpy(a.sun_path + abstract, path, plen + 1);
    socklen_t alen = abstract
        ? (socklen_t)(offsetof(struct sockaddr_un, sun_path) + 1 + plen)
        : (socklen_t)sizeof a;
    bus = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (bus < 0) die("socket() failed");
    if (connect(bus, (struct sockaddr *)&a, alen) < 0) {
        close(bus); bus = -1;
        return 0;
    }

    /* SASL EXTERNAL: the kernel already told the bus who we are, so the only
     * "credential" is our uid, in hex, after a leading NUL byte. */
    char uid[16], hex[33], line[256];
    int un = snprintf(uid, sizeof uid, "%u", (unsigned)getuid());
    for (int i = 0; i < un; i++) snprintf(hex + i * 2, 3, "%02x", (unsigned char)uid[i]);
    unsigned char req[96];
    size_t rn = 0;
    req[rn++] = 0;
    rn += (size_t)snprintf((char *)req + rn, sizeof req - rn, "AUTH EXTERNAL %s\r\n", hex);
    size_t off = 0;
    while (off < rn) {
        ssize_t k = write(bus, req + off, rn - off);
        if (k < 0) { if (errno == EINTR) continue; die("write to the session bus failed"); }
        off += (size_t)k;
    }
    if (!bus_line(line, sizeof line) || strncmp(line, "OK", 2))
        die("the session bus rejected our credentials: %s", line);
    static const char begin[] = "BEGIN\r\n";
    off = 0;
    while (off < sizeof begin - 1) {
        ssize_t k = write(bus, begin + off, sizeof begin - 1 - off);
        if (k < 0) { if (errno == EINTR) continue; die("write to the session bus failed"); }
        off += (size_t)k;
    }

    struct dbus_msg m;                 /* nothing is routed before Hello */
    d_call(&m, "org.freedesktop.DBus", "/org/freedesktop/DBus",
           "org.freedesktop.DBus", "Hello", NULL);
    if (!d_wait(d_send(&m), NULL, 0)) die("the session bus refused Hello: %s", bus_err);
    return 1;
}

/* ------------------------------------------- remote desktop key delivery */

static int rd_warned;

static void rd_key(uint32_t code, uint32_t state) {
    struct dbus_msg m;
    d_call(&m, rd->service, rd_path, rd_iface, "NotifyKeyboardKeycode", "ub");
    d_u32(&m, code); d_u32(&m, state ? 1 : 0);
    /* Waiting for the reply costs a round trip per key, but it is what
     * surfaces a refusal and keeps us from filling the bus socket. */
    if (!d_wait(d_send(&m), NULL, 0) && !rd_warned++)
        warn("the compositor rejected a key event: %s", bus_err);
}

/* End the session rather than leaving it to the socket being closed: utype_run
 * is a library call and may well not be the last thing the process does. */
static void rd_done(void) {
    struct dbus_msg m;
    d_call(&m, rd->service, rd_path, rd_iface, "Stop", NULL);
    d_wait(d_send(&m), NULL, 0);
    close(bus); bus = -1;
}

static int run_remote_desktop(void) {
    if (!bus_connect()) { vlog("no session bus to ask about remote desktop"); return 0; }

    /* An unowned name means only that this is not that desktop, so keep
     * looking; anything else is a real refusal and worth reporting. */
    struct dbus_msg m;
    for (size_t i = 0; i < sizeof rd_flavors / sizeof *rd_flavors; i++) {
        rd = &rd_flavors[i];
        vlog("asking %s for a session", rd->service);
        d_call(&m, rd->service, rd->path, rd->service, "CreateSession", NULL);
        if (d_wait(d_send(&m), rd_path, sizeof rd_path)) break;
        vlog("  %s", bus_err);
        if (!strstr(bus_err, "ServiceUnknown") && !strstr(bus_err, "NameHasNoOwner") &&
            !strstr(bus_err, "NoReply"))
            die("%s refused a remote-desktop session: %s", rd->service, bus_err);
        rd = NULL;
    }
    if (!rd) return 0;
    if (!rd_path[0]) die("%s returned an empty session path", rd->service);
    snprintf(rd_iface, sizeof rd_iface, "%s.Session", rd->service);

    d_call(&m, rd->service, rd_path, rd_iface, "Start", NULL);
    if (!d_wait(d_send(&m), NULL, 0))
        die("%s would not start the session: %s", rd->service, bus_err);
    vlog("using remote-desktop (%s, session %s)", rd->service, rd_path);

    fetch_seat_keymap();
    emit_key = rd_key;
    emit_done = rd_done;
    run_keycode_commands();
    return 1;
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

enum { EV_SYN = 0, EV_KEY = 1, SYN_REPORT = 0 };

static int yd_sock = -1;

/* struct input_event, spelled out: linux/input.h would otherwise be the only
 * kernel header this file needs. `long` matches the kernel's timestamp width
 * on both 32- and 64-bit, which is what makes the size match ydotoold's. */
struct evdev_event {
    long sec, usec;
    uint16_t type, code;
    int32_t value;
};

static int yd_try(const char *path) {
    struct sockaddr_un a = { 0 };
    a.sun_family = AF_UNIX;
    if (!path || !*path || strlen(path) >= sizeof a.sun_path) return 0;
    memcpy(a.sun_path, path, strlen(path) + 1);
    yd_sock = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if (yd_sock < 0) return 0;
    if (connect(yd_sock, (struct sockaddr *)&a, sizeof a) == 0) {
        vlog("using uinput (ydotoold, socket %s)", path);
        return 1;
    }
    vlog("no ydotoold on %s", path);
    close(yd_sock); yd_sock = -1;
    return 0;
}

/* Where the daemon puts its socket: under XDG_RUNTIME_DIR when it runs as the
 * user, in /tmp when it runs as root. ydotool's own client picks one of those
 * and gives up; trying both costs nothing and finds a root daemon from inside
 * a user session. */
static int yd_connect(void) {
    const char *env = getenv("YDOTOOL_SOCKET");
    if (env && *env) return yd_try(env);
    const char *rt = getenv("XDG_RUNTIME_DIR");
    char path[256];
    if (rt && *rt) {
        snprintf(path, sizeof path, "%s/.ydotool_socket", rt);
        if (yd_try(path)) return 1;
    }
    return yd_try("/tmp/.ydotool_socket");
}

static void yd_emit(uint16_t type, uint16_t code, int32_t value) {
    struct evdev_event ev = { 0, 0, type, code, value };
    for (;;) {
        ssize_t n = write(yd_sock, &ev, sizeof ev);
        if (n < 0 && errno == EINTR) continue;
        if (n != (ssize_t)sizeof ev) die("write to the ydotoold socket failed");
        return;
    }
}

/* a key event, and the SYN_REPORT that makes the kernel act on it */
static void yd_key(uint32_t code, uint32_t state) {
    yd_emit(EV_KEY, (uint16_t)code, state ? 1 : 0);
    yd_emit(EV_SYN, SYN_REPORT, 0);
}

static void yd_done(void) { close(yd_sock); yd_sock = -1; }

static int run_ydotool(void) {
    if (!yd_connect()) return 0;
    fetch_seat_keymap();
    emit_key = yd_key;
    emit_done = yd_done;
    run_keycode_commands();
    return 1;
}

/* --------------------------------------------------- backend E: X11/XTEST */
/*
 * Spoken straight over the display socket, like the Wayland side: no libX11,
 * no libXtst, no -dev packages. XTEST fakes *keycodes*, not keysyms, so a
 * keysym is typed on whichever key of the current layout carries it, adding
 * Shift when the layout only offers it shifted. Whatever the layout cannot
 * produce at all is parked on a keycode it leaves empty
 * (ChangeKeyboardMapping) and the mapping is put back before exiting, so the
 * backend can still type any character on any layout.
 *
 * Remapping is the slow path: it makes the server broadcast a MappingNotify
 * and every client re-read the keyboard, so it is worth pausing around, and
 * worth avoiding. Ordinary text never triggers it.
 */

/* core request opcodes we use */
enum { X_QUERY_POINTER = 38, X_GET_INPUT_FOCUS = 43, X_QUERY_EXTENSION = 98,
       X_CHANGE_KEYBOARD_MAPPING = 100, X_GET_KEYBOARD_MAPPING = 101 };
/* XTEST's minor opcode, and the event types FakeInput takes */
enum { XT_FAKE_INPUT = 2, X_KEY_PRESS = 2, X_KEY_RELEASE = 3 };

#define X_LOCK_MASK  2      /* Caps Lock, in a KEYBUTMASK */
#define X_FAMILY_LOCAL 256  /* Xauthority family for local connections */
#define REMAP_MS 50         /* grace for clients to re-read a changed keymap */

static int xsock = -1;
static int x_dnum;               /* display number out of $DISPLAY */
static uint8_t xtest_op;         /* XTEST's major opcode on this server */
static uint32_t x_root;          /* root window of the first screen */
static int x_min_kc, x_max_kc, x_syms;      /* keyboard mapping geometry */
static uint32_t *x_map;          /* the mapping as we found it, never modified */
static uint8_t *x_touched;       /* keycodes we rebound, ascending */
static int x_ntouched;
static uint8_t *x_slotkc;        /* keycode holding each collected slot */
static int x_nslotkc;
static uint8_t x_spill;          /* keycode rebound on demand when slots run out */
static uint32_t x_spill_ks;
static int x_restoring, x_dead;  /* teardown must not die() or block */

static void xput16(unsigned char *b, uint16_t v) { memcpy(b, &v, 2); }
static void xput32(unsigned char *b, uint32_t v) { memcpy(b, &v, 4); }
static uint16_t xget16(const unsigned char *b) { uint16_t v; memcpy(&v, b, 2); return v; }
static uint32_t xget32(const unsigned char *b) { uint32_t v; memcpy(&v, b, 4); return v; }

/* the keysym at shift level `i` of keycode `kc`, as the server reported it */
#define X_SYM(kc, i) x_map[((kc) - x_min_kc) * x_syms + (i)]

static void x_write(const unsigned char *buf, size_t size) {
    size_t off = 0;
    while (off < size) {
        ssize_t n = write(xsock, buf + off, size - off);
        if (n < 0) {
            if (errno == EINTR) continue;
            if (x_restoring) { x_dead = 1; return; }
            die("write to the X server failed");
        }
        off += (size_t)n;
    }
}

static void x_read(unsigned char *buf, size_t size) {
    size_t off = 0;
    while (off < size) {
        ssize_t n = read(xsock, buf + off, size - off);
        if (n < 0) { if (errno == EINTR) continue; die("read from the X server failed"); }
        if (n == 0) die("the X server closed the connection");
        off += (size_t)n;
    }
}

static void x_drain(uint32_t words) {
    unsigned char junk[256];
    for (uint32_t left = words * 4; left; ) {
        size_t take = left < sizeof junk ? left : sizeof junk;
        x_read(junk, take);
        left -= (uint32_t)take;
    }
}

/* Read packets until the reply we are waiting for turns up. Events arrive
 * unasked (every ChangeKeyboardMapping makes the server broadcast a
 * MappingNotify) and are discarded; errors are fatal. Any data past the
 * 32-byte header is handed back through *extra for the caller to free. */
static void x_reply(unsigned char *hdr, unsigned char **extra) {
    if (extra) *extra = NULL;
    for (;;) {
        x_read(hdr, 32);
        if (hdr[0] == 0)                       /* Error */
            die("X error %u on request %u.%u", hdr[1], hdr[10], xget16(hdr + 8));
        if (hdr[0] == 1) {                     /* Reply */
            uint32_t words = xget32(hdr + 4);
            if (!words) return;
            if (!extra) { x_drain(words); return; }
            *extra = malloc((size_t)words * 4);
            if (!*extra) die("out of memory");
            x_read(*extra, (size_t)words * 4);
            return;
        }
        if ((hdr[0] & 0x7f) == 35) x_drain(xget32(hdr + 4));   /* GenericEvent */
    }
}

/* Block until the server has processed everything sent so far. GetInputFocus
 * is the cheapest request that carries a reply, which is what XSync uses. */
static void x_sync(void) {
    if (x_dead) return;
    unsigned char req[4] = { X_GET_INPUT_FOCUS, 0, 0, 0 };
    xput16(req + 2, 1);
    x_write(req, sizeof req);
    unsigned char hdr[32];
    x_reply(hdr, NULL);
}

/* ------------------------------------------------------- X11 connection */

/* Connect to $DISPLAY, "[host]:display[.screen]". An empty or "unix" host
 * means the local socket, anything else (including "localhost", which is how
 * ssh -X forwarding presents itself) means TCP. */
static void x_open(const char *disp) {
    const char *colon = strrchr(disp, ':');
    if (!colon) die("malformed DISPLAY '%s'", disp);
    char host[256];
    size_t hlen = (size_t)(colon - disp);
    if (hlen >= sizeof host) die("malformed DISPLAY '%s'", disp);
    memcpy(host, disp, hlen); host[hlen] = 0;
    x_dnum = atoi(colon + 1);
    if (x_dnum < 0) die("malformed DISPLAY '%s'", disp);

    if (!host[0] || !strcmp(host, "unix")) {
        char path[128];
        int plen = snprintf(path, sizeof path, "/tmp/.X11-unix/X%d", x_dnum);
        struct sockaddr_un a = { 0 };
        a.sun_family = AF_UNIX;
        /* the abstract socket first, the way Xlib probes for it on Linux */
        memcpy(a.sun_path + 1, path, (size_t)plen);
        socklen_t alen = (socklen_t)(offsetof(struct sockaddr_un, sun_path) + 1 + plen);
        xsock = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if (xsock < 0) die("socket() failed");
        if (connect(xsock, (struct sockaddr *)&a, alen) == 0) return;
        close(xsock);
        memset(&a, 0, sizeof a);
        a.sun_family = AF_UNIX;
        memcpy(a.sun_path, path, (size_t)plen + 1);
        xsock = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if (xsock < 0) die("socket() failed");
        if (connect(xsock, (struct sockaddr *)&a, sizeof a) == 0) return;
        die("cannot connect to the X display %s", disp);
    }

    char port[16];
    snprintf(port, sizeof port, "%d", 6000 + x_dnum);
    struct addrinfo hints = { 0 }, *res = NULL;
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(host, port, &hints, &res))
        die("cannot resolve the X display host '%s'", host);
    for (struct addrinfo *ai = res; ai; ai = ai->ai_next) {
        xsock = socket(ai->ai_family, ai->ai_socktype | SOCK_CLOEXEC, ai->ai_protocol);
        if (xsock < 0) continue;
        if (connect(xsock, ai->ai_addr, ai->ai_addrlen) == 0) { freeaddrinfo(res); return; }
        close(xsock); xsock = -1;
    }
    freeaddrinfo(res);
    die("cannot connect to the X display %s", disp);
}

/* one length-prefixed Xauthority field; lengths there are always big-endian */
static int xa_field(FILE *f, unsigned char *dst, size_t cap, uint16_t *len) {
    unsigned char h[2];
    if (fread(h, 1, 2, f) != 2) return 0;
    uint16_t n = (uint16_t)((h[0] << 8) | h[1]);
    if (n > cap) return 0;               /* nonsense entry: stop reading */
    if (n && fread(dst, 1, n, f) != n) return 0;
    *len = n;
    return 1;
}

/* Find this display's MIT-MAGIC-COOKIE-1. A missing or unreadable file is not
 * an error: servers with authorization turned off let us in regardless. */
static int x_cookie(unsigned char *out, uint16_t *outlen) {
    char buf[512];
    const char *path = getenv("XAUTHORITY");
    if (!path || !*path) {
        const char *home = getenv("HOME");
        if (!home) return 0;
        snprintf(buf, sizeof buf, "%s/.Xauthority", home);
        path = buf;
    }
    FILE *f = fopen(path, "rb");
    if (!f) return 0;

    char host[256] = "", want[16];
    gethostname(host, sizeof host - 1);
    snprintf(want, sizeof want, "%d", x_dnum);

    int best = 0;
    while (best < 2) {
        unsigned char fam[2], addr[256], num[32], name[64], data[256];
        uint16_t alen, nlen, mlen, dlen;
        if (fread(fam, 1, 2, f) != 2) break;
        if (!xa_field(f, addr, sizeof addr - 1, &alen)) break;
        if (!xa_field(f, num,  sizeof num  - 1, &nlen)) break;
        if (!xa_field(f, name, sizeof name - 1, &mlen)) break;
        if (!xa_field(f, data, sizeof data,     &dlen)) break;
        addr[alen] = num[nlen] = name[mlen] = 0;
        if (strcmp((char *)name, "MIT-MAGIC-COOKIE-1")) continue;
        if (nlen && strcmp((char *)num, want)) continue;
        /* an entry naming this host beats a generic one, but either will do */
        int score = ((fam[0] << 8 | fam[1]) == X_FAMILY_LOCAL
                     && !strcmp((char *)addr, host)) ? 2 : 1;
        if (score > best) { best = score; memcpy(out, data, dlen); *outlen = dlen; }
    }
    fclose(f);
    return best > 0;
}

/* The connection handshake. The server answers in whatever byte order we ask
 * for, so every field after this one is plain host-order. */
static void x_setup(const unsigned char *cookie, uint16_t clen) {
    static const char proto[] = "MIT-MAGIC-COOKIE-1";
    uint16_t plen = clen ? (uint16_t)(sizeof proto - 1) : 0;
    size_t ppad = (plen + 3u) & ~3u, cpad = (clen + 3u) & ~3u;
    unsigned char req[320] = { 0 };
    uint16_t one = 1;
    if (12 + ppad + cpad > sizeof req) die("the X authorization cookie is too large");
    req[0] = *(unsigned char *)&one ? 'l' : 'B';
    xput16(req + 2, 11); xput16(req + 4, 0);            /* protocol version 11.0 */
    xput16(req + 6, plen); xput16(req + 8, clen);
    memcpy(req + 12, proto, plen);
    memcpy(req + 12 + ppad, cookie, clen);
    x_write(req, 12 + ppad + cpad);

    unsigned char hdr[8];
    x_read(hdr, sizeof hdr);
    uint32_t words = xget16(hdr + 6);
    unsigned char *body = malloc((size_t)words * 4 + 1);
    if (!body) die("out of memory");
    x_read(body, (size_t)words * 4);
    body[words * 4] = 0;
    if (hdr[0] != 1)
        die("the X server refused the connection: %.*s", (int)hdr[1], (char *)body);

    x_min_kc = body[26];
    x_max_kc = body[27];
    if (x_max_kc < x_min_kc) die("the X server reports an empty keycode range");
    if (!body[20]) die("the X server reports no screens");
    /* the screens follow the vendor string and the pixmap formats; a screen
     * opens with its root window, which is all we need from there */
    size_t off = 32 + ((xget16(body + 16) + 3u) & ~3u) + (size_t)body[21] * 8;
    if (off + 4 > (size_t)words * 4) die("malformed X server setup reply");
    x_root = xget32(body + off);
    free(body);
}

static void x_query_xtest(void) {
    static const char nm[] = "XTEST";
    uint16_t n = sizeof nm - 1;
    unsigned char req[16] = { 0 };
    req[0] = X_QUERY_EXTENSION;
    xput16(req + 2, (uint16_t)(2 + ((n + 3u) & ~3u) / 4));
    xput16(req + 4, n);
    memcpy(req + 8, nm, n);
    x_write(req, 8 + ((n + 3u) & ~3u));
    unsigned char hdr[32];
    x_reply(hdr, NULL);
    if (!hdr[8]) die("this X server has no XTEST extension; input cannot be faked");
    xtest_op = hdr[9];
    vlog("XTEST is present, major opcode %u", xtest_op);
}

static void x_read_map(void) {
    int count = x_max_kc - x_min_kc + 1;
    unsigned char req[8] = { 0 };
    req[0] = X_GET_KEYBOARD_MAPPING;
    xput16(req + 2, 2);
    req[4] = (uint8_t)x_min_kc;
    req[5] = (uint8_t)count;
    x_write(req, sizeof req);

    unsigned char hdr[32], *extra;
    x_reply(hdr, &extra);
    x_syms = hdr[1];
    if (x_syms < 1 || !extra) die("the X server returned an empty keyboard mapping");
    if (xget32(hdr + 4) != (uint32_t)count * x_syms) die("malformed keyboard mapping reply");
    x_map = (uint32_t *)(void *)extra;
}

/* The keycode whose plain, unmodified keysym is ks, or 0 if the layout has no
 * such key. Only used for the modifier keys, which we press for real. */
static uint8_t x_find_kc(uint32_t ks) {
    for (int kc = x_min_kc; kc <= x_max_kc; kc++)
        if (X_SYM(kc, 0) == ks) return (uint8_t)kc;
    return 0;
}

/* Where the current layout puts ks: the keycode, with *shift set when it sits
 * at the shifted level. Returns 0 when the layout cannot produce it, which
 * sends the caller off to borrow a keycode instead. Only the unshifted and
 * shifted levels of the first group are considered; deeper levels need a
 * modifier or group switch this backend does not drive, and borrowing is both
 * simpler and more predictable than guessing at them. */
static uint8_t x_layout_kc(uint32_t ks, int *shift) {
    *shift = 0;
    if (!ks) return 0;
    uint8_t kc = x_find_kc(ks);          /* unshifted: no modifier needed */
    if (kc) return kc;
    if (x_syms >= 2)
        for (int c = x_min_kc; c <= x_max_kc; c++)
            if (X_SYM(c, 1) == ks) { *shift = 1; return (uint8_t)c; }
    return 0;
}

/* rebind a contiguous run of keycodes, n keycodes of m keysyms each */
static void x_change_map(int first, int n, const uint32_t *syms, int m) {
    size_t size = 8 + (size_t)n * m * 4;
    unsigned char *req = calloc(1, size);
    if (!req) die("out of memory");
    req[0] = X_CHANGE_KEYBOARD_MAPPING;
    req[1] = (uint8_t)n;
    xput16(req + 2, (uint16_t)(2 + n * m));
    req[4] = (uint8_t)first;
    req[5] = (uint8_t)m;
    memcpy(req + 8, syms, (size_t)n * m * 4);
    x_write(req, size);
    free(req);
}

/* Put every keycode we rebound back the way the server had it. Registered
 * with atexit(), so a fatal error partway through still leaves the user's
 * keyboard alone. */
static void x_restore(void) {
    int n = x_ntouched;
    if (!n) return;
    x_ntouched = 0;   /* claim the work up front: this may re-enter via atexit */
    /* Let whatever has focus translate the keycodes we just sent before the
     * mapping under them moves again; clients read the map on their own clock. */
    nap(REMAP_MS * 2);
    x_restoring = 1;
    for (int i = 0; i < n; ) {
        int run = 1;
        while (i + run < n && x_touched[i + run] == x_touched[i] + run) run++;
        uint32_t *syms = malloc((size_t)run * x_syms * 4);
        if (!syms) break;
        for (int j = 0; j < run; j++)
            memcpy(syms + (size_t)j * x_syms, &X_SYM(x_touched[i + j], 0),
                   (size_t)x_syms * 4);
        x_change_map(x_touched[i], run, syms, x_syms);
        free(syms);
        i += run;
    }
    x_sync();
    x_restoring = 0;
}

/* Park every collected keysym on a keycode the layout does not use, repeating
 * it across the full width of the row. Filling every column makes the key
 * immune to Shift, Caps Lock and the active layout group, and keeping the row
 * exactly as wide as the server's own keysyms_per_keycode means the server
 * never has to resize the map, which would rewrite (and subtly damage) rows
 * we never asked to touch. */
static void x_bind_slots(void) {
    /* Drop the keysyms the layout can already produce: those are typed on
     * their own keys and cost no remapping, which is the common case and the
     * whole reason plain text needs no keymap change at all. */
    int keep = 0;
    for (int i = 0; i < nslots; i++) {
        int sh;
        if (!x_layout_kc(slots[i], &sh)) slots[keep++] = slots[i];
    }
    nslots = keep;

    uint8_t *spare = malloc((size_t)(x_max_kc - x_min_kc + 1));
    if (!spare) die("out of memory");
    int nspare = 0;
    for (int kc = x_min_kc; kc <= x_max_kc; kc++) {
        int used = 0;
        for (int i = 0; i < x_syms && !used; i++) used = X_SYM(kc, i) != 0;
        if (!used) spare[nspare++] = (uint8_t)kc;
    }

    int nbind;
    if (nslots <= nspare) { nbind = nslots; x_spill = 0; }
    else if (nspare > 0)  { nbind = nspare - 1; x_spill = spare[nspare - 1]; }
    else                  { nbind = 0; x_spill = (uint8_t)x_max_kc; }  /* borrow one */

    x_slotkc = calloc((size_t)(nslots > 0 ? nslots : 1), 1);
    x_touched = malloc((size_t)nbind + 1);
    if (!x_slotkc || !x_touched) die("out of memory");
    x_nslotkc = nslots;
    atexit(x_restore);

    for (int i = 0; i < nbind; ) {
        int run = 1;
        while (i + run < nbind && spare[i + run] == spare[i] + run) run++;
        uint32_t *syms = malloc((size_t)run * x_syms * 4);
        if (!syms) die("out of memory");
        for (int j = 0; j < run; j++) {
            for (int col = 0; col < x_syms; col++)
                syms[(size_t)j * x_syms + col] = slots[i + j];
            x_slotkc[i + j] = spare[i + j];
            x_touched[x_ntouched++] = spare[i + j];
        }
        x_change_map(spare[i], run, syms, x_syms);
        free(syms);
        i += run;
    }
    if (x_spill) x_touched[x_ntouched++] = x_spill;
    free(spare);

    if (x_ntouched) {
        vlog("borrowed %d keycode(s) for characters the layout cannot produce",
             x_ntouched);
        x_sync();
        nap(REMAP_MS);   /* toolkits re-read the map when our MappingNotify lands */
    }
}

/* The keycode to press for ks: its own key on the layout where there is one,
 * otherwise a borrowed one, rebinding the spill key if the layout had fewer
 * free keycodes than we needed slots. */
static uint8_t x_kc_for(uint32_t ks, int *shift) {
    uint8_t lk = x_layout_kc(ks, shift);
    if (lk) return lk;
    int slot = slot_for(ks) - 1;
    if (slot < x_nslotkc && x_slotkc[slot]) return x_slotkc[slot];
    *shift = 0;   /* borrowed keys carry the keysym at every level */
    if (!x_spill) { warn("no free keycode left for this key; skipped"); return 0; }
    if (x_spill_ks != ks) {
        uint32_t *row = malloc((size_t)x_syms * 4);
        if (!row) die("out of memory");
        for (int col = 0; col < x_syms; col++) row[col] = ks;
        x_change_map(x_spill, 1, row, x_syms);
        free(row);
        x_sync();
        nap(REMAP_MS);
        x_spill_ks = ks;
    }
    return x_spill;
}

/* ---------------------------------------------------------- X11 typing */

static void x_key(uint8_t kc, int down) {
    if (!kc) return;
    unsigned char req[36] = { 0 };
    req[0] = xtest_op;
    req[1] = XT_FAKE_INPUT;
    xput16(req + 2, 9);
    req[4] = (uint8_t)(down ? X_KEY_PRESS : X_KEY_RELEASE);
    req[5] = kc;
    /* time 0 asks for no server-side delay, root None means the current
     * screen, and device 0 means the core keyboard */
    x_write(req, sizeof req);
}

static void x_tap(uint8_t kc, int delay) {
    x_key(kc, 1); nap(HOLD_MS);
    x_key(kc, 0); nap(HOLD_MS);
    nap(delay);
}

/* the keysyms that carry each modifier, best first */
static const uint32_t *mod_keysyms(int mod) {
    static const uint32_t shift[] = { 0xffe1, 0xffe2, 0 };          /* Shift_L, Shift_R */
    static const uint32_t ctrl[]  = { 0xffe3, 0xffe4, 0 };          /* Control_L, Control_R */
    static const uint32_t alt[]   = { 0xffe9, 0xffea, 0xffe7, 0 };  /* Alt_L, Alt_R, Meta_L */
    static const uint32_t logo[]  = { 0xffeb, 0xffec, 0 };          /* Super_L, Super_R */
    static const uint32_t altgr[] = { 0xfe03, 0xff7e, 0xffea, 0 };  /* ISO_Level3_Shift, Mode_switch, Alt_R */
    static const uint32_t caps[]  = { 0xffe5, 0 };                  /* Caps_Lock */
    switch (mod) {
    case UTYPE_SHIFT: return shift;
    case UTYPE_CTRL:  return ctrl;
    case UTYPE_ALT:   return alt;
    case UTYPE_LOGO:  return logo;
    case UTYPE_ALTGR: return altgr;
    case UTYPE_CAPS:  return caps;
    }
    return NULL;
}

/* A modifier's keycode: a real key from the layout where there is one, else a
 * scratch key holding the keysym, which the server's compatibility map turns
 * back into the modifier. */
static uint8_t x_mod_kc(int mod) {
    const uint32_t *ks = mod_keysyms(mod);
    if (!ks) return 0;
    for (int i = 0; ks[i]; i++) {
        uint8_t kc = x_find_kc(ks[i]);
        if (kc) return kc;
    }
    int sh;
    return x_kc_for(ks[0], &sh);
}

/* Type one keysym: press the key that carries it, holding Shift for the run of
 * the stroke when the layout only offers it shifted and the caller is not
 * already holding Shift itself. */
static void x_stroke(uint32_t ks, int delay) {
    int shift;
    uint8_t kc = x_kc_for(ks, &shift);
    if (!kc) return;
    uint8_t skc = (shift && !(held_mods & UTYPE_SHIFT)) ? x_mod_kc(UTYPE_SHIFT) : 0;
    if (skc) x_key(skc, 1);
    x_key(kc, 1); nap(HOLD_MS);
    x_key(kc, 0); nap(HOLD_MS);
    if (skc) x_key(skc, 0);
    nap(delay);
}

/* Modifiers with no key of their own on this layout need a slot as well. */
static void x_collect_mod_slots(void) {
    for (int i = 0; i < ncmds; i++) {
        if (cmds[i].type != UTYPE_MODPRESS && cmds[i].type != UTYPE_MODRELEASE) continue;
        const uint32_t *ks = mod_keysyms(cmds[i].mod);
        if (!ks) continue;
        int found = 0;
        for (int j = 0; ks[j] && !found; j++) found = x_find_kc(ks[j]) != 0;
        if (!found) slot_for(ks[0]);
    }
}

static int x_caps_on(void) {
    unsigned char req[8] = { 0 };
    req[0] = X_QUERY_POINTER;
    xput16(req + 2, 2);
    xput32(req + 4, x_root);
    x_write(req, sizeof req);
    unsigned char hdr[32];
    x_reply(hdr, NULL);
    return (xget16(hdr + 24) & X_LOCK_MASK) != 0;
}

static void x_run_commands(void) {
    /* Caps Lock would invert letter case, as it does on the fake_input path. */
    uint8_t caps_kc = x_caps_on() ? x_find_kc(0xffe5) : 0;
    if (caps_kc) { x_tap(caps_kc, 0); nap(20); }

    for (int i = 0; i < ncmds; i++) {
        const struct utype_cmd *c = &cmds[i];
        vlog_cmd(c);
        switch (c->type) {
        case UTYPE_TEXT: {
            const unsigned char *p = (const unsigned char *)c->text;
            uint32_t cp;
            for (int adv; (adv = utf8_next(p, &cp)); p += adv) {
                if (cp == '\r') continue;
                uint32_t ks = char_to_keysym(cp);
                char nm[256];
                if (!ks || xkb_keysym_get_name(ks, nm, sizeof nm) <= 0) {
                    warn("U+%04X has no keysym; skipped", cp); continue;
                }
                x_stroke(ks, c->ms);
            }
            break;
        }
        case UTYPE_TAP: x_stroke(c->keysym, c->ms); break;
        case UTYPE_PRESS: case UTYPE_RELEASE: {
            int sh;   /* a held key is held as it lies; level is the caller's business */
            x_key(x_kc_for(c->keysym, &sh), c->type == UTYPE_PRESS);
            break;
        }
        case UTYPE_MODPRESS:
        case UTYPE_MODRELEASE: {
            uint8_t kc = x_mod_kc(c->mod);
            if (!kc) { warn("this modifier has no key on the X server; skipped"); break; }
            if (c->mod == UTYPE_CAPS) x_tap(kc, 0);   /* a lock toggle, not a hold */
            else x_key(kc, c->type == UTYPE_MODPRESS);
            if (c->type == UTYPE_MODPRESS) held_mods |= c->mod;
            else held_mods &= ~c->mod;
            break;
        }
        case UTYPE_SLEEP: nap(c->ms); break;
        }
    }

    if (caps_kc) x_tap(caps_kc, 0);
    x_sync();                        /* make sure the keys land before we exit */
}

static void run_x11(void) {
    const char *disp = getenv("DISPLAY");
    unsigned char cookie[256];
    uint16_t clen = 0;

    vlog("using xtest (X display %s)", disp ? disp : "(unset)");
    x_open(disp);
    x_cookie(cookie, &clen);
    x_setup(cookie, clen);
    x_query_xtest();
    x_read_map();
    collect_slots();          /* x_bind_slots() drops what the layout can type */
    x_collect_mod_slots();
    x_bind_slots();
    x_run_commands();
    x_restore();
}

/* -------------------------------------------------------------- public API */

uint32_t utype_keysym(const char *name) {
    load_xkb_base();
    return xkb_keysym_from_name(name, 1 /* case-insensitive */);
}

enum { P_AUTO, P_VKBD, P_FAKE, P_RD, P_UINPUT, P_XTEST };

static const struct { const char *name; int id; } protocol_names[] = {
    { "auto",             P_AUTO },
    { "virtual-keyboard", P_VKBD },
    { "fake-input",       P_FAKE },
    { "remote-desktop",   P_RD },
    { "uinput",           P_UINPUT },
    { "xtest",            P_XTEST },
};

/* UTYPE_PROTOCOL pins the choice instead of letting the environment decide,
 * which is what the test suite uses and what to reach for when the automatic
 * answer is the wrong one. An unusable choice is an error, not a fallback:
 * asking for something specific and silently getting something else would
 * defeat the point. */
static int forced_protocol(void) {
    const char *p = getenv("UTYPE_PROTOCOL");
    if (!p || !*p) return P_AUTO;
    for (size_t i = 0; i < sizeof protocol_names / sizeof *protocol_names; i++)
        if (!strcasecmp(p, protocol_names[i].name)) {
            vlog("UTYPE_PROTOCOL pins the choice to %s", protocol_names[i].name);
            return protocol_names[i].id;
        }
    die("UTYPE_PROTOCOL: unknown protocol '%s'.\n"
        "       Use one of: auto, virtual-keyboard, fake-input, remote-desktop,\n"
        "       uinput, xtest.", p);
    return P_AUTO;   /* not reached */
}

void utype_run(const struct utype_cmd *c, int n) {
    cmds = c; ncmds = n;
    load_xkb_base();

    int want = forced_protocol();
    const char *wd = getenv("WAYLAND_DISPLAY"), *xd = getenv("DISPLAY");
    int have_wl = wd && *wd, have_x11 = xd && *xd;
    vlog("WAYLAND_DISPLAY=%s, DISPLAY=%s",
         have_wl ? wd : "(unset)", have_x11 ? xd : "(unset)");

    if (want == P_XTEST || (want == P_AUTO && !have_wl)) {
        if (!have_x11)
            die(want == P_XTEST
                ? "UTYPE_PROTOCOL=xtest, but DISPLAY is not set"
                : "no display server found: neither WAYLAND_DISPLAY nor DISPLAY is set");
        run_x11();
        return;
    }

    /* Everything else rides on the Wayland connection, uinput included: that
     * backend presses keycodes, so it still needs the compositor's keymap to
     * know which one carries each character. */
    if (!wl_connect(want != P_AUTO || !have_x11)) {
        warn("cannot connect to the Wayland display; falling back to X11");
        run_x11();       /* wl_connect only returns on failure when X11 is there */
        return;
    }
    get_globals();
    vlog("compositor offers: virtual-keyboard %s, fake_input %s, wl_seat %s",
         yesno(have_vkm), yesno(have_fake), yesno(have_seat));

    switch (want) {
    case P_VKBD:
        if (!have_vkm)
            die("UTYPE_PROTOCOL=virtual-keyboard, but this compositor does not offer it");
        run_virtual_keyboard();
        return;
    case P_FAKE:
        run_fake_input();
        return;
    case P_RD:
        if (!run_remote_desktop())
            die("UTYPE_PROTOCOL=remote-desktop, but nothing answered on the session bus");
        return;
    case P_UINPUT:
        if (!run_ydotool())
            die("UTYPE_PROTOCOL=uinput, but ydotoold is not reachable");
        return;
    case P_AUTO:
        break;
    }

    /* KWin hands out fake_input only after a first run installs the .desktop
     * file that asks for it, so trust the desktop's own name over the absence
     * of the global. */
    if (have_vkm) run_virtual_keyboard();
    else if (have_fake || is_kde()) run_fake_input();
    else if (!run_remote_desktop() && !run_ydotool()) die(NO_INPUT_PROTOCOL);
}
