// Measures which keyboard layouts type Latin letters, for isLatin() in
// src/compositor.cpp. A developer tool, not part of the build:
//
//   cc -O2 -o /tmp/xkb-latin tools/xkb-latin.c $(pkg-config --cflags --libs xkbcommon)
//   /tmp/xkb-latin > tests/xkb-latin.tsv
//
// Every layout and variant listed in xkeyboard-config's evdev.lst is compiled
// with libxkbcommon; the output is one line per layout: its name and how many
// of the letters a-z the base level (no Shift, no AltGr) of the main key block
// types. The tests check isLatin() against this file; when they fail after an
// xkeyboard-config update, update the tables in isLatin().

#include <stdio.h>
#include <string.h>
#include <xkbcommon/xkbcommon.h>

static const char *keys[] = {
    "TLDE", "BKSL", "LSGT",
    "AE01", "AE02", "AE03", "AE04", "AE05", "AE06", "AE07", "AE08", "AE09", "AE10", "AE11", "AE12",
    "AD01", "AD02", "AD03", "AD04", "AD05", "AD06", "AD07", "AD08", "AD09", "AD10", "AD11", "AD12",
    "AC01", "AC02", "AC03", "AC04", "AC05", "AC06", "AC07", "AC08", "AC09", "AC10", "AC11",
    "AB01", "AB02", "AB03", "AB04", "AB05", "AB06", "AB07", "AB08", "AB09", "AB10",
};

static void measure(struct xkb_context *context, const char *layout, const char *variant) {
    struct xkb_rule_names names = {"evdev", "pc105", layout, variant, NULL};
    struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, &names, 0);
    if (!keymap) {
        return; // "custom" and the like: nothing to compile
    }
    int seen[26] = {0}, letters = 0;
    for (size_t i = 0; i < sizeof keys / sizeof *keys; i++) {
        const xkb_keysym_t *syms;
        xkb_keycode_t key = xkb_keymap_key_by_name(keymap, keys[i]);
        int count = xkb_keymap_key_get_syms_by_level(keymap, key, 0, 0, &syms);
        for (int j = 0; j < count; j++) {
            xkb_keysym_t sym = xkb_keysym_to_lower(syms[j]);
            if (sym >= XKB_KEY_a && sym <= XKB_KEY_z && !seen[sym - XKB_KEY_a]++) {
                letters++;
            }
        }
    }
    if (*variant) {
        printf("%s(%s)\t%d\n", layout, variant, letters);
    } else {
        printf("%s\t%d\n", layout, letters);
    }
    xkb_keymap_unref(keymap);
}

int main(void) {
    FILE *list = fopen("/usr/share/X11/xkb/rules/evdev.lst", "r");
    struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    if (!list || !context) {
        return 1;
    }
    xkb_context_set_log_level(context, XKB_LOG_LEVEL_CRITICAL);
    char line[512], section[32] = "", name[128], layout[128];
    while (fgets(line, sizeof line, list)) {
        if (line[0] == '!') {
            sscanf(line, "! %31s", section);
        } else if (!strcmp(section, "layout") && sscanf(line, " %127s", name) == 1) {
            measure(context, name, "");
        } else if (!strcmp(section, "variant") &&
                   sscanf(line, " %127s %127[^:]:", name, layout) == 2) {
            measure(context, layout, name);
        }
    }
    xkb_context_unref(context);
    fclose(list);
    return 0;
}
