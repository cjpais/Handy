#include <CoreFoundation/CoreFoundation.h>
#include <dispatch/dispatch.h>
#include <dlfcn.h>
#include <stdint.h>

static const char *MEDIA_REMOTE_PATH = "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote";
static const int64_t MEDIA_REMOTE_TIMEOUT_NANOS = 2LL * NSEC_PER_SEC;

typedef void (^MRAnyPlayingCallback)(Boolean is_playing);
typedef void (*MRGetAnyApplicationIsPlayingFn)(dispatch_queue_t queue, MRAnyPlayingCallback callback);
typedef void (*MRSendCommandFn)(uint32_t command, CFDictionaryRef options);

static void *open_media_remote(void) {
    return dlopen(MEDIA_REMOTE_PATH, RTLD_LAZY | RTLD_LOCAL);
}

int32_t media_remote_any_application_is_playing(void) {
    void *handle = open_media_remote();
    if (handle == NULL) {
        return -1;
    }

    MRGetAnyApplicationIsPlayingFn function =
        (MRGetAnyApplicationIsPlayingFn)dlsym(handle, "MRMediaRemoteGetAnyApplicationIsPlaying");
    if (function == NULL) {
        dlclose(handle);
        return -2;
    }

    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
    __block int32_t result = -4;

    function(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(Boolean is_playing) {
        result = is_playing ? 1 : 0;
        dispatch_semaphore_signal(semaphore);
    });

    long wait_result = dispatch_semaphore_wait(
        semaphore,
        dispatch_time(DISPATCH_TIME_NOW, MEDIA_REMOTE_TIMEOUT_NANOS)
    );

    dlclose(handle);

    if (wait_result != 0) {
        return -3;
    }

    return result;
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
