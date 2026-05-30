#import <Cocoa/Cocoa.h>
#include <CoreFoundation/CoreFoundation.h>
#include <dispatch/dispatch.h>
#include <dlfcn.h>
#include <IOKit/hidsystem/ev_keymap.h>
#include <stdint.h>
#include <unistd.h>

static const char *MEDIA_REMOTE_PATH = "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote";

typedef void (*MRSendCommandFn)(uint32_t command, CFDictionaryRef options);

static void *open_media_remote(void) {
    return dlopen(MEDIA_REMOTE_PATH, RTLD_LAZY | RTLD_LOCAL);
}

int32_t media_remote_send_command(int32_t command) {
    void *handle = open_media_remote();
    if (handle == NULL) {
        return -1;
    }

    MRSendCommandFn function = (MRSendCommandFn)dlsym(handle, "MRMediaRemoteSendCommand");
    if (function == NULL) {
        dlclose(handle);
        return -2;
    }

    function((uint32_t)command, NULL);
    dlclose(handle);
    return 0;
}

int32_t media_remote_send_play_pause_key(void) {
    @autoreleasepool {
        int key_down_data = (NX_KEYTYPE_PLAY << 16) | (NX_KEYDOWN << 8);
        int key_up_data = (NX_KEYTYPE_PLAY << 16) | (NX_KEYUP << 8);

        NSEvent *key_down = [NSEvent otherEventWithType:NSEventTypeSystemDefined
                                               location:NSMakePoint(0, 0)
                                          modifierFlags:0
                                              timestamp:0
                                           windowNumber:0
                                                context:nil
                                                subtype:NX_SUBTYPE_AUX_CONTROL_BUTTONS
                                                  data1:key_down_data
                                                  data2:-1];
        NSEvent *key_up = [NSEvent otherEventWithType:NSEventTypeSystemDefined
                                             location:NSMakePoint(0, 0)
                                        modifierFlags:0
                                            timestamp:0
                                         windowNumber:0
                                              context:nil
                                              subtype:NX_SUBTYPE_AUX_CONTROL_BUTTONS
                                                data1:key_up_data
                                                data2:-1];

        if (key_down == nil || key_up == nil || [key_down CGEvent] == NULL || [key_up CGEvent] == NULL) {
            return -4;
        }

        CGEventPost(kCGHIDEventTap, [key_down CGEvent]);
        usleep(10000);
        CGEventPost(kCGHIDEventTap, [key_up CGEvent]);
    }
    return 0;
}
