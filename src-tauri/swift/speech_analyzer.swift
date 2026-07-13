import AVFoundation
import Dispatch
import Foundation
import Speech

// MARK: - Swift implementation for Apple SpeechAnalyzer transcription
// This file is compiled via Cargo build script for Apple Silicon targets.
// Mirrors the apple_intelligence.swift bridge: blocking @_cdecl entry points
// that run the async SpeechAnalyzer API on a detached task.

private typealias ResponsePointer = UnsafeMutablePointer<SpeechAnalyzerResponse>

private func duplicateCString(_ text: String) -> UnsafeMutablePointer<CChar>? {
    return text.withCString { basePointer in
        guard let duplicated = strdup(basePointer) else {
            return nil
        }
        return duplicated
    }
}

private func makeResponse() -> ResponsePointer {
    let responsePtr = ResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(to: SpeechAnalyzerResponse(text: nil, success: 0, error_message: nil))
    return responsePtr
}

// Thread-safe container to pass results from async task back to calling thread
private final class ResultBox: @unchecked Sendable {
    var text: String?
    var error: String?
}

/// Run an async operation to completion on the calling (FFI) thread.
private func runBlocking(_ operation: @escaping @Sendable () async throws -> String)
    -> ResponsePointer
{
    let responsePtr = makeResponse()
    let semaphore = DispatchSemaphore(value: 0)
    let box = ResultBox()

    Task.detached(priority: .userInitiated) {
        defer { semaphore.signal() }
        do {
            box.text = try await operation()
        } catch {
            box.error = error.localizedDescription
        }
    }

    semaphore.wait()

    if let text = box.text {
        responsePtr.pointee.text = duplicateCString(text)
        responsePtr.pointee.success = 1
    } else {
        responsePtr.pointee.error_message = duplicateCString(box.error ?? "Unknown error")
    }
    return responsePtr
}

@available(macOS 26.0, *)
private func resolveLocale(_ identifier: String) async throws -> Locale {
    let requested = Locale(identifier: identifier)
    let supported = await SpeechTranscriber.supportedLocales
    if let match = supported.first(where: {
        $0.identifier(.bcp47) == requested.identifier(.bcp47)
    }) {
        return match
    }
    // Bare language codes ("en"): prefer the system's own region variant so a
    // US user gets en-US spelling, then fall back to any same-language locale.
    let sameLanguage = supported.filter {
        $0.language.languageCode == requested.language.languageCode
    }
    if let match = sameLanguage.first(where: { $0.region == Locale.current.region }) {
        return match
    }
    if let match = sameLanguage.first {
        return match
    }
    throw NSError(
        domain: "SpeechAnalyzerBridge", code: 1,
        userInfo: [
            NSLocalizedDescriptionKey:
                "Locale \(identifier) is not supported by SpeechTranscriber."
        ])
}

/// Download and install the on-device assets for the transcriber if missing.
@available(macOS 26.0, *)
private func ensureAssets(for transcriber: SpeechTranscriber) async throws {
    if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
        try await request.downloadAndInstall()
    }
}

@available(macOS 26.0, *)
private func transcribeSamples(_ samples: [Float], localeId: String) async throws -> String {
    let locale = try await resolveLocale(localeId)
    let transcriber = SpeechTranscriber(
        locale: locale,
        transcriptionOptions: [],
        reportingOptions: [],
        attributeOptions: []
    )
    try await ensureAssets(for: transcriber)

    let analyzer = SpeechAnalyzer(modules: [transcriber])

    guard
        let inputFormat = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: 16000, channels: 1, interleaved: false)
    else {
        throw NSError(
            domain: "SpeechAnalyzerBridge", code: 2,
            userInfo: [NSLocalizedDescriptionKey: "Failed to create input audio format."])
    }
    guard
        let inputBuffer = AVAudioPCMBuffer(
            pcmFormat: inputFormat, frameCapacity: AVAudioFrameCount(samples.count))
    else {
        throw NSError(
            domain: "SpeechAnalyzerBridge", code: 3,
            userInfo: [NSLocalizedDescriptionKey: "Failed to allocate audio buffer."])
    }
    inputBuffer.frameLength = AVAudioFrameCount(samples.count)
    samples.withUnsafeBufferPointer { src in
        inputBuffer.floatChannelData![0].update(from: src.baseAddress!, count: samples.count)
    }

    // The analyzer dictates the audio format; convert our 16 kHz mono input to it.
    let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [transcriber])
    let buffer: AVAudioPCMBuffer
    if let analyzerFormat = analyzerFormat, analyzerFormat != inputFormat {
        buffer = try convert(inputBuffer, to: analyzerFormat)
    } else {
        buffer = inputBuffer
    }

    // Collect final results concurrently while the analyzer consumes the input.
    let resultsTask = Task {
        var text = ""
        for try await result in transcriber.results where result.isFinal {
            text += String(result.text.characters)
        }
        return text
    }

    let (inputSequence, inputBuilder) = AsyncStream.makeStream(of: AnalyzerInput.self)
    inputBuilder.yield(AnalyzerInput(buffer: buffer))
    inputBuilder.finish()

    try await analyzer.start(inputSequence: inputSequence)
    try await analyzer.finalizeAndFinishThroughEndOfInput()

    return try await resultsTask.value
        .trimmingCharacters(in: .whitespacesAndNewlines)
}

@available(macOS 26.0, *)
private func convert(_ input: AVAudioPCMBuffer, to format: AVAudioFormat) throws -> AVAudioPCMBuffer
{
    guard let converter = AVAudioConverter(from: input.format, to: format) else {
        throw NSError(
            domain: "SpeechAnalyzerBridge", code: 4,
            userInfo: [NSLocalizedDescriptionKey: "Failed to create audio converter."])
    }
    let ratio = format.sampleRate / input.format.sampleRate
    let capacity = AVAudioFrameCount(Double(input.frameLength) * ratio) + 1024
    guard let output = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: capacity) else {
        throw NSError(
            domain: "SpeechAnalyzerBridge", code: 5,
            userInfo: [NSLocalizedDescriptionKey: "Failed to allocate conversion buffer."])
    }

    var fed = false
    var conversionError: NSError?
    converter.convert(to: output, error: &conversionError) { _, outStatus in
        if fed {
            outStatus.pointee = .endOfStream
            return nil
        }
        fed = true
        outStatus.pointee = .haveData
        return input
    }
    if let conversionError = conversionError {
        throw conversionError
    }
    return output
}

@_cdecl("is_speech_analyzer_available")
public func isSpeechAnalyzerAvailable() -> Int32 {
    guard #available(macOS 26.0, *) else {
        return 0
    }
    return 1
}

@_cdecl("speech_analyzer_prepare")
public func speechAnalyzerPrepare(
    _ localeId: UnsafePointer<CChar>
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    let swiftLocaleId = String(cString: localeId)
    guard #available(macOS 26.0, *) else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechAnalyzer requires macOS 26 or newer.")
        return responsePtr
    }
    return runBlocking {
        let locale = try await resolveLocale(swiftLocaleId)
        let transcriber = SpeechTranscriber(
            locale: locale,
            transcriptionOptions: [],
            reportingOptions: [],
            attributeOptions: []
        )
        try await ensureAssets(for: transcriber)
        return ""
    }
}

@_cdecl("speech_analyzer_transcribe")
public func speechAnalyzerTranscribe(
    _ samples: UnsafePointer<Float>,
    _ sampleCount: Int32,
    _ localeId: UnsafePointer<CChar>
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    let swiftLocaleId = String(cString: localeId)
    guard #available(macOS 26.0, *) else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechAnalyzer requires macOS 26 or newer.")
        return responsePtr
    }
    let sampleArray = Array(UnsafeBufferPointer(start: samples, count: Int(sampleCount)))
    return runBlocking {
        try await transcribeSamples(sampleArray, localeId: swiftLocaleId)
    }
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
