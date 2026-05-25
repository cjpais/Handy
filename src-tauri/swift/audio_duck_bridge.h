#ifndef audio_duck_bridge_h
#define audio_duck_bridge_h

#ifdef __cplusplus
extern "C" {
#endif

// Start ducking all audio processes at the given level (0.0 = silent, 1.0 = full).
// Returns 0 on success, negative on error.
int audio_duck_start(float duck_level);

// Stop ducking: fade back to full volume then clean up.
// Returns 0 on success, negative on error.
int audio_duck_stop(void);

// Check if ducking is currently active. Returns 1 if active, 0 if not.
int audio_duck_is_active(void);

#ifdef __cplusplus
}
#endif

#endif /* audio_duck_bridge_h */
