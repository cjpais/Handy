#include <stdlib.h>
#include <string.h>

typedef struct {
    char* response;
    int success;
    char* error_message;
} AppleLLMResponse;

int is_apple_intelligence_available() {
    return 0;
}

AppleLLMResponse* process_text_with_system_prompt_apple(
    const char* system_prompt,
    const char* user_content,
    int max_tokens
) {
    AppleLLMResponse* response = (AppleLLMResponse*)malloc(sizeof(AppleLLMResponse));
    if (response == NULL) {
        return NULL;
    }
    response->response = NULL;
    response->success = 0;
    response->error_message = strdup("Apple Intelligence is not available in this build (Xcode SDK missing).");
    return response;
}

void free_apple_llm_response(AppleLLMResponse* response) {
    if (response == NULL) {
        return;
    }
    if (response->response != NULL) {
        free(response->response);
    }
    if (response->error_message != NULL) {
        free(response->error_message);
    }
    free(response);
}
