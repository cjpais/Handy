/* SPDX-License-Identifier: MIT */
/*
 * libutype - fake keyboard input on Linux. Public API.
 *
 * Build an array of actions and hand it to utype_run(). See utype.c for how
 * the work is done and which Wayland compositors and X11 servers are
 * supported.
 */
#ifndef UTYPE_H
#define UTYPE_H

#include <stdint.h>

/* Modifier bits, OR them together. Values match wtype's wire protocol. */
enum {
    UTYPE_SHIFT = 1,
    UTYPE_CAPS  = 2,
    UTYPE_CTRL  = 4,
    UTYPE_ALT   = 8,
    UTYPE_LOGO  = 64,
    UTYPE_ALTGR = 128,
};

/* Action types for struct utype_cmd. */
enum {
    UTYPE_TEXT,       /* type the UTF-8 string in .text */
    UTYPE_TAP,        /* press and release .keysym */
    UTYPE_PRESS,      /* press and hold .keysym */
    UTYPE_RELEASE,    /* release .keysym */
    UTYPE_MODPRESS,   /* press and hold modifier .mod */
    UTYPE_MODRELEASE, /* release modifier .mod */
    UTYPE_SLEEP,      /* wait .ms milliseconds */
};

struct utype_cmd {
    int type;           /* one of the UTYPE_* actions above */
    const char *text;   /* UTYPE_TEXT */
    uint32_t keysym;    /* UTYPE_TAP / PRESS / RELEASE */
    int mod;            /* UTYPE_MODPRESS / MODRELEASE */
    int ms;             /* UTYPE_SLEEP, and the per-key delay for TEXT/TAP */
};

/* Look up an XKB keysym by name (case-insensitive), e.g. "Return", "comma".
 * Returns 0 if the name is unknown. */
uint32_t utype_keysym(const char *name);

/* Narrate what utype does, and how it picks a protocol, on stderr. Off by
 * default; the command-line tool turns it on for -v. */
void utype_verbose(int on);

/* Run a sequence of actions against whichever session the environment
 * describes: a Wayland compositor when WAYLAND_DISPLAY is set and reachable,
 * otherwise the X11 server named by DISPLAY. The protocol is chosen
 * automatically unless UTYPE_PROTOCOL names one (auto, virtual-keyboard,
 * fake-input, remote-desktop, uinput, xtest), in which case an unusable choice
 * is an error. On a fatal error it prints "utype: ..." to stderr and exits the
 * process. */
void utype_run(const struct utype_cmd *cmds, int ncmds);

#endif
