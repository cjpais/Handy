#import <Cocoa/Cocoa.h>
#include <IOKit/hidsystem/ev_keymap.h>
#include <stdint.h>
#include <unistd.h>

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
