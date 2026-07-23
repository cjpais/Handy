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

@_cdecl("speech_analyzer_supported_locales")
public func speechAnalyzerSupportedLocales() -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
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

@_cdecl("speech_analyzer_stream_start")
public func speechAnalyzerStreamStart(
    _ localeId: UnsafePointer<CChar>,
    _ streamOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    streamOut.pointee = nil
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
}

@_cdecl("speech_analyzer_stream_feed")
public func speechAnalyzerStreamFeed(
    _ stream: UnsafeMutableRawPointer?,
    _ samples: UnsafePointer<Float>,
    _ sampleCount: Int32
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
}

@_cdecl("speech_analyzer_stream_snapshot")
public func speechAnalyzerStreamSnapshot(
    _ stream: UnsafeMutableRawPointer?
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
}

@_cdecl("speech_analyzer_stream_finish")
public func speechAnalyzerStreamFinish(
    _ stream: UnsafeMutableRawPointer?
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
}

@_cdecl("speech_analyzer_stream_cancel")
public func speechAnalyzerStreamCancel(
    _ stream: UnsafeMutableRawPointer?
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    return failureResponse(
        "SpeechAnalyzer is not available in this build (SDK requirement not met).")
}

@_cdecl("free_speech_analyzer_stream")
public func freeSpeechAnalyzerStream(_ stream: UnsafeMutableRawPointer?) {}

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
