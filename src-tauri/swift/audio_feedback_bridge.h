#ifndef AUDIO_FEEDBACK_BRIDGE_H
#define AUDIO_FEEDBACK_BRIDGE_H

#include <stdint.h>

/// Plays a sound file through the system output device (avoids AirPods Handoff)
/// @param file_path Path to the audio file to play
/// @param volume Volume level from 0.0 to 1.0
/// @return 0 on success, negative error code on failure
int32_t play_sound_via_system_output(const char *file_path, float volume);

/// Checks if the system output device is available
/// @return 1 if available, 0 if not
int32_t is_system_output_available(void);

#endif /* AUDIO_FEEDBACK_BRIDGE_H */
