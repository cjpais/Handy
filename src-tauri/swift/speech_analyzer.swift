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

@available(macOS 26.0, *)
private final class StreamBox: @unchecked Sendable {
    var session: StreamingSession?
}

private struct StreamResultEvent: Codable {
    let revision: UInt64
    let elapsedMs: UInt64
    let isFinal: Bool
    let text: String
}

private struct StreamSnapshot: Codable {
    let revision: UInt64
    let committed: String
    let tentative: String
    let events: [StreamResultEvent]
}

/// Result state shared between SpeechTranscriber's AsyncSequence consumer and
/// the synchronous FFI polling calls made by Rust's streaming worker.
private final class StreamingResultState: @unchecked Sendable {
    private let lock = NSLock()
    private var revision: UInt64 = 0
    private let startedAt = ProcessInfo.processInfo.systemUptime
    private var committed = ""
    private var tentative = ""
    private var pendingEvents: [StreamResultEvent] = []
    private var error: String?

    func apply(text: String, isFinal: Bool) {
        lock.lock()
        defer { lock.unlock() }

        if isFinal {
            committed += text
            tentative = ""
        } else {
            // Volatile results repeatedly replace the same not-yet-final audio
            // range. Only one volatile suffix follows the committed prefix.
            tentative = text
        }
        revision &+= 1
        pendingEvents.append(
            StreamResultEvent(
                revision: revision,
                elapsedMs: UInt64(
                    max(0, (ProcessInfo.processInfo.systemUptime - startedAt) * 1000)),
                isFinal: isFinal,
                text: text
            ))
    }

    func record(error: Error) {
        lock.lock()
        self.error = error.localizedDescription
        lock.unlock()
    }

    func snapshot() throws -> StreamSnapshot {
        lock.lock()
        defer { lock.unlock() }
        if let error {
            throw NSError(
                domain: "SpeechAnalyzerBridge", code: 10,
                userInfo: [NSLocalizedDescriptionKey: error])
        }
        let events = pendingEvents
        pendingEvents.removeAll(keepingCapacity: true)
        return StreamSnapshot(
            revision: revision,
            committed: committed,
            tentative: tentative,
            events: events
        )
    }
}

/// A long-lived SpeechAnalyzer session. Audio arrives incrementally through the
/// AsyncStream while final and volatile transcription results are consumed on a
/// separate task. Chunk boundaries transport audio; they do not reset model
/// context because every buffer belongs to this one analyzer session.
@available(macOS 26.0, *)
private final class StreamingSession: @unchecked Sendable {
    private let analyzer: SpeechAnalyzer
    private let continuation: AsyncStream<AnalyzerInput>.Continuation
    private let resultsTask: Task<String, Error>
    private let resultState: StreamingResultState
    private let inputFormat: AVAudioFormat
    private let analyzerFormat: AVAudioFormat?
    private var isFinished = false

    private init(
        analyzer: SpeechAnalyzer,
        continuation: AsyncStream<AnalyzerInput>.Continuation,
        resultsTask: Task<String, Error>,
        resultState: StreamingResultState,
        inputFormat: AVAudioFormat,
        analyzerFormat: AVAudioFormat?
    ) {
        self.analyzer = analyzer
        self.continuation = continuation
        self.resultsTask = resultsTask
        self.resultState = resultState
        self.inputFormat = inputFormat
        self.analyzerFormat = analyzerFormat
    }

    static func start(localeId: String) async throws -> StreamingSession {
        let locale = try await resolveLocale(localeId)
        let transcriber = SpeechTranscriber(
            locale: locale,
            transcriptionOptions: [],
            // Request revisable live text while retaining the full-context,
            // highest-quality transcription path. Apple controls when these
            // volatile revisions are delivered.
            reportingOptions: [.volatileResults],
            attributeOptions: []
        )
        let analyzer = SpeechAnalyzer(modules: [transcriber])
        guard
            let inputFormat = AVAudioFormat(
                commonFormat: .pcmFormatFloat32,
                sampleRate: 16000,
                channels: 1,
                interleaved: false)
        else {
            throw NSError(
                domain: "SpeechAnalyzerBridge", code: 2,
                userInfo: [NSLocalizedDescriptionKey: "Failed to create input audio format."])
        }
        let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(
            compatibleWith: [transcriber], considering: inputFormat)
        let (inputSequence, continuation) = AsyncStream.makeStream(of: AnalyzerInput.self)
        let resultState = StreamingResultState()
        let resultsTask = Task<String, Error> {
            do {
                var finalText = ""
                for try await result in transcriber.results {
                    let text = String(result.text.characters)
                    resultState.apply(text: text, isFinal: result.isFinal)
                    if result.isFinal {
                        finalText += text
                    }
                }
                return finalText
            } catch {
                resultState.record(error: error)
                throw error
            }
        }

        do {
            try await analyzer.start(inputSequence: inputSequence)
        } catch {
            continuation.finish()
            resultsTask.cancel()
            _ = try? await resultsTask.value
            throw error
        }

        return StreamingSession(
            analyzer: analyzer,
            continuation: continuation,
            resultsTask: resultsTask,
            resultState: resultState,
            inputFormat: inputFormat,
            analyzerFormat: analyzerFormat
        )
    }

    func feed(_ samples: [Float]) throws {
        guard !isFinished else {
            throw NSError(
                domain: "SpeechAnalyzerBridge", code: 11,
                userInfo: [NSLocalizedDescriptionKey: "SpeechAnalyzer stream is already finished."])
        }
        guard !samples.isEmpty else { return }

        let inputBuffer = try makeAudioBuffer(samples, format: inputFormat)
        let buffer: AVAudioPCMBuffer
        if let analyzerFormat, analyzerFormat != inputFormat {
            buffer = try convert(inputBuffer, to: analyzerFormat)
        } else {
            buffer = inputBuffer
        }

        if case .terminated = continuation.yield(AnalyzerInput(buffer: buffer)) {
            throw NSError(
                domain: "SpeechAnalyzerBridge", code: 12,
                userInfo: [NSLocalizedDescriptionKey: "SpeechAnalyzer input stream terminated."])
        }
    }

    func snapshotJSON() throws -> String {
        let data = try JSONEncoder().encode(resultState.snapshot())
        guard let json = String(data: data, encoding: .utf8) else {
            throw NSError(
                domain: "SpeechAnalyzerBridge", code: 13,
                userInfo: [NSLocalizedDescriptionKey: "Failed to encode stream snapshot."])
        }
        return json
    }

    func finish() async throws -> String {
        guard !isFinished else {
            throw NSError(
                domain: "SpeechAnalyzerBridge", code: 14,
                userInfo: [NSLocalizedDescriptionKey: "SpeechAnalyzer stream is already finished."])
        }
        isFinished = true
        continuation.finish()

        do {
            try await analyzer.finalizeAndFinishThroughEndOfInput()
            return try await resultsTask.value
                .trimmingCharacters(in: .whitespacesAndNewlines)
        } catch {
            resultsTask.cancel()
            await analyzer.cancelAndFinishNow()
            _ = try? await resultsTask.value
            throw error
        }
    }

    func cancel() async {
        guard !isFinished else { return }
        isFinished = true
        continuation.finish()
        resultsTask.cancel()
        await analyzer.cancelAndFinishNow()
        _ = try? await resultsTask.value
    }
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
    if let match = await SpeechTranscriber.supportedLocale(equivalentTo: requested) {
        return match
    }
    throw NSError(
        domain: "SpeechAnalyzerBridge", code: 1,
        userInfo: [
            NSLocalizedDescriptionKey:
                "Locale \(identifier) is not supported by SpeechTranscriber."
        ])
}

@available(macOS 26.0, *)
private func reservation(_ reservedLocale: Locale, matches selectedLocale: Locale) async -> Bool {
    let selectedIdentifier = selectedLocale.identifier(.bcp47)
    if reservedLocale.identifier(.bcp47) == selectedIdentifier {
        return true
    }

    // Apple may return a canonical variant rather than the identifier supplied
    // when the reservation was created. Compare through SpeechTranscriber's
    // own equivalence resolver before deciding that the slot belongs elsewhere.
    return await SpeechTranscriber.supportedLocale(equivalentTo: reservedLocale)?
        .identifier(.bcp47) == selectedIdentifier
}

/// Ensure there is a reservation slot for the locale, then download and install
/// its on-device assets if missing. Blocks until the OS finishes; Handy's model
/// loading state covers the wait in the UI.
@available(macOS 26.0, *)
private func ensureAssets(
    for transcriber: SpeechTranscriber,
    locale: Locale
) async throws {
    let selectedIdentifier = locale.identifier(.bcp47)
    let reservedLocales = await AssetInventory.reservedLocales
    var selectedIsReserved = false
    for reservedLocale in reservedLocales {
        if await reservation(reservedLocale, matches: locale) {
            selectedIsReserved = true
            break
        }
    }

    // AssetInstallationRequest reserves automatically, but throws when the app
    // has used every slot. Release non-selected reservations only until a slot
    // is free; retaining spare reservations avoids needless language churn.
    if !selectedIsReserved && reservedLocales.count >= AssetInventory.maximumReservedLocales {
        var reservationCount = reservedLocales.count
        for reservedLocale in reservedLocales
        where reservedLocale.identifier(.bcp47) != selectedIdentifier {
            guard reservationCount >= AssetInventory.maximumReservedLocales else { break }
            if await AssetInventory.release(reservedLocale: reservedLocale) {
                reservationCount -= 1
            }
        }
    }

    if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
        try await request.downloadAndInstall()
    }
}

private let batchChunkFrames = 16_000 // One second at Handy's 16 kHz input rate.

@available(macOS 26.0, *)
private func makeAudioBuffer(_ samples: [Float], format: AVAudioFormat) throws -> AVAudioPCMBuffer {
    guard
        let buffer = AVAudioPCMBuffer(
            pcmFormat: format, frameCapacity: AVAudioFrameCount(samples.count))
    else {
        throw NSError(
            domain: "SpeechAnalyzerBridge", code: 3,
            userInfo: [NSLocalizedDescriptionKey: "Failed to allocate audio buffer."])
    }
    buffer.frameLength = AVAudioFrameCount(samples.count)
    samples.withUnsafeBufferPointer { source in
        buffer.floatChannelData![0].update(from: source.baseAddress!, count: samples.count)
    }
    return buffer
}

@available(macOS 26.0, *)
private func transcribeSamples(_ samples: [Float], localeId: String) async throws -> String {
    guard !samples.isEmpty else { return "" }

    let locale = try await resolveLocale(localeId)
    let transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
    let analyzer = SpeechAnalyzer(modules: [transcriber])

    guard
        let inputFormat = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: 16000, channels: 1, interleaved: false)
    else {
        throw NSError(
            domain: "SpeechAnalyzerBridge", code: 2,
            userInfo: [NSLocalizedDescriptionKey: "Failed to create input audio format."])
    }
    let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(
        compatibleWith: [transcriber], considering: inputFormat)

    // Consume final results concurrently while the analyzer processes the input.
    let resultsTask = Task<String, Error> {
        var text = ""
        for try await result in transcriber.results where result.isFinal {
            text += String(result.text.characters)
        }
        return text
    }

    let (inputSequence, inputBuilder) = AsyncStream.makeStream(of: AnalyzerInput.self)

    do {
        // Start the autonomous consumer before producing input so a long batch
        // does not accumulate a second complete copy in AsyncStream's queue.
        try await analyzer.start(inputSequence: inputSequence)

        // Feed manageable buffers into one analyzer session. Supplying a
        // multi-minute recording as one AVAudioPCMBuffer causes SpeechAnalyzer
        // to silently omit early audio; sequence chunking preserves the complete
        // recording without resetting model context between chunks.
        var offset = 0
        while offset < samples.count {
            let end = min(offset + batchChunkFrames, samples.count)
            let sourceBuffer = try makeAudioBuffer(
                Array(samples[offset..<end]), format: inputFormat)
            let buffer: AVAudioPCMBuffer
            if let analyzerFormat, analyzerFormat != inputFormat {
                buffer = try convert(sourceBuffer, to: analyzerFormat)
            } else {
                buffer = sourceBuffer
            }
            if case .terminated = inputBuilder.yield(AnalyzerInput(buffer: buffer)) {
                throw NSError(
                    domain: "SpeechAnalyzerBridge", code: 12,
                    userInfo: [NSLocalizedDescriptionKey: "SpeechAnalyzer input stream terminated."])
            }
            offset = end
        }
        inputBuilder.finish()
        try await analyzer.finalizeAndFinishThroughEndOfInput()

        return try await resultsTask.value
            .trimmingCharacters(in: .whitespacesAndNewlines)
    } catch {
        inputBuilder.finish()
        resultsTask.cancel()
        await analyzer.cancelAndFinishNow()
        _ = try? await resultsTask.value
        throw error
    }
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
    let status = converter.convert(to: output, error: &conversionError) { _, outStatus in
        if fed {
            outStatus.pointee = .endOfStream
            return nil
        }
        fed = true
        outStatus.pointee = .haveData
        return input
    }
    if let conversionError {
        throw conversionError
    }
    if status == .error {
        throw NSError(
            domain: "SpeechAnalyzerBridge", code: 6,
            userInfo: [NSLocalizedDescriptionKey: "Audio conversion failed."])
    }
    return output
}

@_cdecl("is_speech_analyzer_available")
public func isSpeechAnalyzerAvailable() -> Int32 {
    guard #available(macOS 26.0, *) else {
        return 0
    }
    return SpeechTranscriber.isAvailable ? 1 : 0
}

@_cdecl("speech_analyzer_supported_locales")
public func speechAnalyzerSupportedLocales() -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    guard #available(macOS 26.0, *) else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechAnalyzer requires macOS 26 or newer.")
        return responsePtr
    }
    guard SpeechTranscriber.isAvailable else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechTranscriber is not available on this device.")
        return responsePtr
    }
    return runBlocking {
        let locales = await SpeechTranscriber.supportedLocales
            .map { $0.identifier(.bcp47) }
            .sorted()
        return locales.joined(separator: "\n")
    }
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
    guard SpeechTranscriber.isAvailable else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechTranscriber is not available on this device.")
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
        try await ensureAssets(for: transcriber, locale: locale)

        // Success text doubles as a diagnostic notice for the Rust side to
        // log. Handy always feeds 16 kHz mono Float32; if the analyzer ever
        // prefers a different format, the transcribe/stream paths convert
        // per-chunk with independent converters, which is only safe while
        // this never actually engages — so leave a trace when it does.
        guard
            let inputFormat = AVAudioFormat(
                commonFormat: .pcmFormatFloat32, sampleRate: 16000, channels: 1,
                interleaved: false),
            let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(
                compatibleWith: [transcriber], considering: inputFormat),
            analyzerFormat != inputFormat
        else {
            return ""
        }
        return
            "analyzer prefers \(Int(analyzerFormat.sampleRate)) Hz, "
            + "\(analyzerFormat.channelCount) ch, commonFormat \(analyzerFormat.commonFormat.rawValue) "
            + "over the 16 kHz mono Float32 feed; per-chunk audio conversion is engaged"
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
    guard SpeechTranscriber.isAvailable else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechTranscriber is not available on this device.")
        return responsePtr
    }
    let sampleArray = Array(UnsafeBufferPointer(start: samples, count: Int(sampleCount)))
    return runBlocking {
        try await transcribeSamples(sampleArray, localeId: swiftLocaleId)
    }
}

@_cdecl("speech_analyzer_stream_start")
public func speechAnalyzerStreamStart(
    _ localeId: UnsafePointer<CChar>,
    _ streamOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    streamOut.pointee = nil
    let swiftLocaleId = String(cString: localeId)
    guard #available(macOS 26.0, *) else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechAnalyzer requires macOS 26 or newer.")
        return responsePtr
    }
    guard SpeechTranscriber.isAvailable else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString(
            "SpeechTranscriber is not available on this device.")
        return responsePtr
    }

    let box = StreamBox()
    let response = runBlocking {
        box.session = try await StreamingSession.start(localeId: swiftLocaleId)
        return ""
    }
    if response.pointee.success == 1, let session = box.session {
        streamOut.pointee = Unmanaged.passRetained(session).toOpaque()
    }
    return response
}

@_cdecl("speech_analyzer_stream_feed")
public func speechAnalyzerStreamFeed(
    _ stream: UnsafeMutableRawPointer?,
    _ samples: UnsafePointer<Float>,
    _ sampleCount: Int32
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    guard #available(macOS 26.0, *), let stream else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString("Invalid SpeechAnalyzer stream.")
        return responsePtr
    }
    let session = Unmanaged<StreamingSession>.fromOpaque(stream).takeUnretainedValue()
    let sampleArray = Array(UnsafeBufferPointer(start: samples, count: Int(sampleCount)))
    return runBlocking {
        try session.feed(sampleArray)
        return ""
    }
}

@_cdecl("speech_analyzer_stream_snapshot")
public func speechAnalyzerStreamSnapshot(
    _ stream: UnsafeMutableRawPointer?
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    guard #available(macOS 26.0, *), let stream else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString("Invalid SpeechAnalyzer stream.")
        return responsePtr
    }
    let session = Unmanaged<StreamingSession>.fromOpaque(stream).takeUnretainedValue()
    return runBlocking {
        try session.snapshotJSON()
    }
}

@_cdecl("speech_analyzer_stream_finish")
public func speechAnalyzerStreamFinish(
    _ stream: UnsafeMutableRawPointer?
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    guard #available(macOS 26.0, *), let stream else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString("Invalid SpeechAnalyzer stream.")
        return responsePtr
    }
    let session = Unmanaged<StreamingSession>.fromOpaque(stream).takeUnretainedValue()
    return runBlocking {
        try await session.finish()
    }
}

@_cdecl("speech_analyzer_stream_cancel")
public func speechAnalyzerStreamCancel(
    _ stream: UnsafeMutableRawPointer?
) -> UnsafeMutablePointer<SpeechAnalyzerResponse> {
    guard #available(macOS 26.0, *), let stream else {
        let responsePtr = makeResponse()
        responsePtr.pointee.error_message = duplicateCString("Invalid SpeechAnalyzer stream.")
        return responsePtr
    }
    let session = Unmanaged<StreamingSession>.fromOpaque(stream).takeUnretainedValue()
    return runBlocking {
        await session.cancel()
        return ""
    }
}

@_cdecl("free_speech_analyzer_stream")
public func freeSpeechAnalyzerStream(_ stream: UnsafeMutableRawPointer?) {
    guard #available(macOS 26.0, *), let stream else { return }
    Unmanaged<StreamingSession>.fromOpaque(stream).release()
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
