#include <CoreFoundation/CoreFoundation.h>
#include <dispatch/dispatch.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdio.h>

typedef void (*MRGetIsPlayingFn)(dispatch_queue_t queue, void (^completion)(bool isPlaying));

static const char *MEDIA_REMOTE_PATH = "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote";

void handy_media_is_playing(void) {
    void *handle = dlopen(MEDIA_REMOTE_PATH, RTLD_NOW | RTLD_GLOBAL);
    if (handle == NULL) {
        fprintf(stderr, "Failed to load MediaRemote framework\n");
        exit(2);
    }

    MRGetIsPlayingFn get_is_playing =
        (MRGetIsPlayingFn)dlsym(handle, "MRMediaRemoteGetNowPlayingApplicationIsPlaying");
    if (get_is_playing == NULL) {
        fprintf(stderr, "MediaRemote is-playing symbol was not found\n");
        exit(3);
    }

    __block bool callback_called = false;
    __block bool is_playing = false;
    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);

    get_is_playing(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(bool result) {
        is_playing = result;
        callback_called = true;
        dispatch_semaphore_signal(semaphore);
    });

    long wait_result = dispatch_semaphore_wait(
        semaphore,
        dispatch_time(DISPATCH_TIME_NOW, 500 * NSEC_PER_MSEC)
    );

    if (wait_result != 0 || !callback_called) {
        fprintf(stderr, "Timed out waiting for MediaRemote is-playing state\n");
        exit(4);
    }

    printf(is_playing ? "true\n" : "false\n");
}
