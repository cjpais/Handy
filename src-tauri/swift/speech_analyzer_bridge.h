#ifndef speech_analyzer_bridge_h
#define speech_analyzer_bridge_h

// C-compatible function declarations for the SpeechAnalyzer Swift bridge

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    char* text;
    int success; // 0 for failure, 1 for success
    char* error_message; // Only valid when success = 0
} SpeechAnalyzerResponse;

// Check if the SpeechAnalyzer API is available on this device (macOS 26+)
int is_speech_analyzer_available(void);

// Return the device's supported BCP-47 locales, separated by newlines.
SpeechAnalyzerResponse* speech_analyzer_supported_locales(void);

// Ensure the on-device speech assets for the locale are installed
// (triggers an OS-managed download if needed). Blocks until done.
// On success, text optionally carries a diagnostic notice worth logging
// (empty when there is nothing to report).
SpeechAnalyzerResponse* speech_analyzer_prepare(const char* locale_id);

// Transcribe 16 kHz mono f32 PCM. Blocks until transcription completes.
SpeechAnalyzerResponse* speech_analyzer_transcribe(const float* samples, int sample_count, const char* locale_id);

// Long-lived streaming session. Audio buffers are fed incrementally into one
// SpeechAnalyzer context; snapshots contain JSON with committed/tentative text.
typedef void* SpeechAnalyzerStreamHandle;
SpeechAnalyzerResponse* speech_analyzer_stream_start(const char* locale_id, SpeechAnalyzerStreamHandle* stream_out);
SpeechAnalyzerResponse* speech_analyzer_stream_feed(SpeechAnalyzerStreamHandle stream, const float* samples, int sample_count);
SpeechAnalyzerResponse* speech_analyzer_stream_snapshot(SpeechAnalyzerStreamHandle stream);
SpeechAnalyzerResponse* speech_analyzer_stream_finish(SpeechAnalyzerStreamHandle stream);
SpeechAnalyzerResponse* speech_analyzer_stream_cancel(SpeechAnalyzerStreamHandle stream);
void free_speech_analyzer_stream(SpeechAnalyzerStreamHandle stream);

// Free memory allocated by the response
void free_speech_analyzer_response(SpeechAnalyzerResponse* response);

#ifdef __cplusplus
}
#endif

#endif /* speech_analyzer_bridge_h */
