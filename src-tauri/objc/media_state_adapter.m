#include <CoreFoundation/CoreFoundation.h>
#include <dispatch/dispatch.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdio.h>

typedef void (*MRGetNowPlayingInfoFn)(dispatch_queue_t queue, void (^completion)(CFDictionaryRef info));

static const char *MEDIA_REMOTE_PATH = "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote";

void handy_media_is_playing(void) {
    void *handle = dlopen(MEDIA_REMOTE_PATH, RTLD_NOW | RTLD_GLOBAL);
    if (handle == NULL) {
        fprintf(stderr, "Failed to load MediaRemote framework\n");
        exit(2);
    }

    MRGetNowPlayingInfoFn get_now_playing_info =
        (MRGetNowPlayingInfoFn)dlsym(handle, "MRMediaRemoteGetNowPlayingInfo");
    if (get_now_playing_info == NULL) {
        fprintf(stderr, "MediaRemote now-playing info symbol was not found\n");
        exit(3);
    }

    __block bool callback_called = false;
    __block bool is_playing = false;
    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);

    get_now_playing_info(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(CFDictionaryRef info) {
        is_playing = false;
        if (info != NULL) {
            CFTypeRef playback_rate_ref =
                CFDictionaryGetValue(info, CFSTR("kMRMediaRemoteNowPlayingInfoPlaybackRate"));
            if (playback_rate_ref != NULL && CFGetTypeID(playback_rate_ref) == CFNumberGetTypeID()) {
                double playback_rate = 0.0;
                if (CFNumberGetValue((CFNumberRef)playback_rate_ref, kCFNumberDoubleType, &playback_rate)) {
                    is_playing = playback_rate > 0.01;
                }
            }
        }
        callback_called = true;
        dispatch_semaphore_signal(semaphore);
    });

    long wait_result = dispatch_semaphore_wait(
        semaphore,
        dispatch_time(DISPATCH_TIME_NOW, 500 * NSEC_PER_MSEC)
    );

    if (wait_result != 0 || !callback_called) {
        fprintf(stderr, "Timed out waiting for MediaRemote now-playing info\n");
        exit(4);
    }

    printf(is_playing ? "true\n" : "false\n");
}
