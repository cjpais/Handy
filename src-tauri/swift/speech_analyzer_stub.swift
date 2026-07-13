import Foundation

// Stub implementation when the SpeechAnalyzer API is not available in the
// build environment (SDK older than macOS 26). Compiled via Cargo build
// script as a fallback so the exported symbols always exist.

private func failureResponse(_ message: String) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    let responsePtr = UnsafeMutablePointer<SpeechAnalyzerResponse>.allocate(capacity: 1)
    responsePtr.initialize(to: SpeechAnalyzerResponse(text: nil, success: 0, error_message: nil))
    responsePtr.pointee.error_message = strdup(message)
    return responsePtr
}

@_cdecl("is_speech_analyzer_available")
public func isSpeechAnalyzerAvailable() -> Int32 {
    return 0
}

@_cdecl("speech_analyzer_prepare")
public func speechAnalyzerPrepare(
    _ localeId: UnsafePointer<CChar>
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
}

@_cdecl("speech_analyzer_transcribe")
public func speechAnalyzerTranscribe(
    _ samples: UnsafePointer<Float>,
    _ sampleCount: Int32,
    _ localeId: UnsafePointer<CChar>
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
}

@_cdecl("free_speech_analyzer_response")
public func freeSpeechAnalyzerResponse(
    _ response: UnsafeMutablePointer<SpeechAnalyzerResponse>?
) {
    guard let response = response else { return }

    if let textStr = response.pointee.text {
        free(UnsafeMutablePointer(mutating: textStr))
    }

    if let errorStr = response.pointee.error_message {
        free(UnsafeMutablePointer(mutating: errorStr))
    }

    response.deallocate()
}
