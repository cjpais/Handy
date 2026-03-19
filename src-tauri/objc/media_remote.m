#include <CoreFoundation/CoreFoundation.h>
#include <dispatch/dispatch.h>
#include <dlfcn.h>
#include <stdint.h>

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
