#ifndef macos_ocr_bridge_h
#define macos_ocr_bridge_h

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    char* text;
    int success; // 0 for failure, 1 for success
    char* error_message; // Only valid when success = 0
} OCRTextResponse;

// Check whether screen capture permission is currently granted.
int macos_ocr_preflight_screen_capture_access(void);

// Prompt for screen capture permission (system prompt shown if needed).
int macos_ocr_request_screen_capture_access(void);

// Capture the frontmost window and return OCR text.
OCRTextResponse* macos_ocr_capture_frontmost_window_text(void);

// Free memory allocated by an OCR response.
void macos_ocr_free_response(OCRTextResponse* response);

#ifdef __cplusplus
}
#endif

#endif /* macos_ocr_bridge_h */
