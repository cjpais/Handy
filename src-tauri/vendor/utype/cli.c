/* SPDX-License-Identifier: MIT */
/*
 * utype - a drop-in wtype replacement, built on libutype.
 *
 * It parses the same command line as wtype (https://github.com/atx/wtype) into
 * a list of actions and hands them to utype_run().
 */
#define _GNU_SOURCE
#include "utype.h"
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <string.h>
#include <strings.h>
#include <unistd.h>

static void die(const char *fmt, ...) {
    va_list ap; va_start(ap, fmt);
    fprintf(stderr, "utype: "); vfprintf(stderr, fmt, ap);
    va_end(ap); fputc('\n', stderr); exit(1);
}

static struct utype_cmd *cmds;
static int ncmds;

static int name_to_mod(const char *n) {
    if (!strcasecmp(n, "shift"))    return UTYPE_SHIFT;
    if (!strcasecmp(n, "capslock")) return UTYPE_CAPS;
    if (!strcasecmp(n, "ctrl"))     return UTYPE_CTRL;
    if (!strcasecmp(n, "logo"))     return UTYPE_LOGO;
    if (!strcasecmp(n, "win"))      return UTYPE_LOGO;
    if (!strcasecmp(n, "alt"))      return UTYPE_ALT;
    if (!strcasecmp(n, "altgr"))    return UTYPE_ALTGR;
    return 0;
}

/* parse a non-negative millisecond count; -1 on anything invalid */
static int parse_ms(const char *s) {
    if (!*s) return -1;
    int v = 0;
    for (const char *p = s; *p; p++) {
        if (*p < '0' || *p > '9') return -1;
        v = v * 10 + (*p - '0');
    }
    return v;
}

static uint32_t key_from_name(const char *n) {
    uint32_t ks = utype_keysym(n);
    if (!ks) die("unknown key '%s'", n);
    return ks;
}

/* Parse the command line the way wtype does. */
static void parse_args(int argc, char **argv) {
    cmds = calloc(argc, sizeof *cmds);
    int have_stdin = 0, delay = 0, prefix_space = 0, raw = 0;
    for (int i = 1; i < argc; i++) {
        struct utype_cmd *c = &cmds[ncmds];
        if (!raw && !strcmp(argv[i], "--")) { raw = 1; continue; }
        if (!raw && !strcmp(argv[i], "-")) {
            if (have_stdin) die("the stdin placeholder can only appear once");
            have_stdin = 1;
            c->type = UTYPE_TEXT; c->text = NULL; c->ms = delay;
            ncmds++; prefix_space = 0; continue;
        }
        if (!raw && argv[i][0] == '-') {
            if (!strcmp(argv[i], "-v")) { utype_verbose(1); continue; }
            if (i == argc - 1) die("missing argument to %s", argv[i]);
            const char *a = argv[i + 1];
            if (!strcmp(argv[i], "-M") || !strcmp(argv[i], "-m")) {
                c->type = argv[i][1] == 'M' ? UTYPE_MODPRESS : UTYPE_MODRELEASE;
                c->mod = name_to_mod(a);
                if (!c->mod) die("invalid modifier name '%s'", a);
            } else if (!strcmp(argv[i], "-P") || !strcmp(argv[i], "-p")) {
                c->type = argv[i][1] == 'P' ? UTYPE_PRESS : UTYPE_RELEASE;
                c->keysym = key_from_name(a);
            } else if (!strcmp(argv[i], "-k")) {
                c->type = UTYPE_TAP; c->keysym = key_from_name(a); c->ms = delay;
            } else if (!strcmp(argv[i], "-s")) {
                c->type = UTYPE_SLEEP; c->ms = parse_ms(a);
                if (c->ms <= 0) die("invalid sleep time '%s'", a);
            } else if (!strcmp(argv[i], "-d")) {
                delay = parse_ms(a);
                if (delay <= 0) die("invalid delay '%s'", a);
                i++; prefix_space = 0; continue;   /* -d sets state, no command */
            } else {
                die("unknown parameter %s", argv[i]);
            }
            i++; ncmds++; prefix_space = 0; continue;
        }
        /* plain text: consecutive text arguments are separated by a space */
        c->type = UTYPE_TEXT; c->ms = delay;
        if (prefix_space) {
            char *t = malloc(strlen(argv[i]) + 2);
            t[0] = ' '; strcpy(t + 1, argv[i]);
            c->text = t;
        } else {
            c->text = strdup(argv[i]);
        }
        ncmds++; prefix_space = 1;
    }
}

static char *read_stdin(void) {
    size_t cap = 4096, len = 0;
    char *t = malloc(cap);
    size_t r;
    while ((r = fread(t + len, 1, cap - len, stdin)) > 0) {
        len += r;
        if (len == cap) { cap *= 2; t = realloc(t, cap); }
    }
    t[len] = 0;
    return t;
}

/* replace any stdin placeholder command's text with what stdin holds */
static void resolve_stdin(void) {
    char *text = NULL;
    for (int i = 0; i < ncmds; i++)
        if (cmds[i].type == UTYPE_TEXT && !cmds[i].text) {
            if (!text) text = read_stdin();
            cmds[i].text = text;
        }
}

int main(int argc, char **argv) {
    /* AppImages (e.g. Handy) may point these at their own bundle, which can
     * break libraries we or our child processes load. We do not need them. */
    unsetenv("LD_LIBRARY_PATH");
    unsetenv("LD_PRELOAD");

    if (argc < 2 && isatty(0))
        die("usage: %s [-M mod] [-m mod] [-P key] [-p key] [-k key]\n"
            "              [-s ms] [-d ms] [-v] [--] <text> ...   (\"-\" reads stdin)\n"
            "       -v narrates what it does; UTYPE_PROTOCOL pins the protocol.", argv[0]);

    parse_args(argc, argv);
    if (argc < 2) {                       /* bare "utype" with piped input: read stdin */
        cmds = calloc(1, sizeof *cmds);
        cmds[0].type = UTYPE_TEXT; cmds[0].text = NULL; ncmds = 1;
    }
    resolve_stdin();

    utype_run(cmds, ncmds);
    return 0;
}
