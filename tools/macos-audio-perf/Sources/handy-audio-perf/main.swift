import AppKit
import AVFoundation
import Carbon.HIToolbox
import Darwin

// MARK: - Timing (mach ticks only)
//
// Everything internal is in raw mach ticks (same units as
// `mach_absolute_time()` and `AVAudioTime.hostTime`). We convert to ms only
// when we write a log line. Mixing units — e.g. `DispatchTime.uptimeNanoseconds`
// for t0 and `hostTime` for the sample timestamp — causes UInt64 underflow
// on Apple Silicon because the two clocks tick at different rates.

private var timebase: mach_timebase_info_data_t = {
    var tb = mach_timebase_info_data_t()
    mach_timebase_info(&tb)
    return tb
}()

@inline(__always)
func nowTicks() -> UInt64 {
    mach_absolute_time()
}

@inline(__always)
func ticksToMs(_ ticks: UInt64) -> Double {
    // ticks * numer / denom → nanoseconds
    Double(ticks) * Double(timebase.numer) / Double(timebase.denom) / 1_000_000.0
}

@inline(__always)
func msSince(_ t0: UInt64) -> Double {
    ticksToMs(nowTicks() &- t0)
}

@inline(__always)
func msBetween(_ a: UInt64, _ b: UInt64) -> Double {
    ticksToMs(b &- a)
}

/// Signed version of msBetween: returns a negative value when `b` is before `a`.
/// In warm mode the engine buffers audio, so the first sample delivered to a
/// freshly-installed tap can legitimately precede the keypress — we want to
/// report that as negative ms, not UInt64-underflow garbage.
@inline(__always)
func msBetweenSigned(_ a: UInt64, _ b: UInt64) -> Double {
    if b >= a {
        return ticksToMs(b &- a)
    } else {
        return -ticksToMs(a &- b)
    }
}

// MARK: - Modes

enum WarmMode: String {
    case cold  // new AVAudioEngine built every press
    case warm  // engine built once, start/stop cycled per press

    static func parse(_ args: [String]) -> WarmMode {
        if args.contains("--warm") { return .warm }
        return .cold
    }
}

// MARK: - Auto mode config

struct AutoConfig {
    let iterations: Int
    let minHold: Double
    let maxHold: Double
    let idle: Double

    static func parse(_ args: [String]) -> AutoConfig? {
        guard args.contains("--auto") else { return nil }
        func flag(_ name: String, default def: Double) -> Double {
            for a in args where a.hasPrefix("\(name)=") {
                if let v = Double(a.dropFirst(name.count + 1)) { return v }
            }
            return def
        }
        return AutoConfig(
            iterations: Int(flag("--iterations", default: 5)),
            minHold:    flag("--min-hold",   default: 1.0),
            maxHold:    flag("--max-hold",   default: 5.0),
            idle:       flag("--idle",       default: 1.0)
        )
    }
}

// MARK: - Tone generation

/// Build a short sine-tone PCM buffer. Generated in-memory so file I/O never
/// appears on the measured path.
func makeToneBuffer(sampleRate: Double = 48_000,
                    duration: Double = 0.12,
                    frequency: Double = 880) -> AVAudioPCMBuffer
{
    guard let format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 1) else {
        fatalError("Failed to create AVAudioFormat")
    }
    let frameCount = AVAudioFrameCount(sampleRate * duration)
    guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frameCount) else {
        fatalError("Failed to allocate AVAudioPCMBuffer")
    }
    buffer.frameLength = frameCount
    let channel = buffer.floatChannelData![0]
    let fadeFrames = Int(sampleRate * 0.005)  // 5ms fade in/out to avoid click
    for i in 0..<Int(frameCount) {
        let t = Double(i) / sampleRate
        var envelope: Float = 1.0
        if i < fadeFrames {
            envelope = Float(Double(i) / Double(fadeFrames))
        } else if i > Int(frameCount) - fadeFrames {
            envelope = Float(Double(Int(frameCount) - i) / Double(fadeFrames))
        }
        channel[i] = Float(sin(2.0 * .pi * frequency * t)) * 0.25 * envelope
    }
    return buffer
}

// MARK: - CSV logging

final class CSVLog {
    private let handle: FileHandle
    private let url: URL

    init(path: String) {
        url = URL(fileURLWithPath: path)
        let fm = FileManager.default
        let needsHeader = !fm.fileExists(atPath: path)
        if needsHeader {
            fm.createFile(atPath: path, contents: nil)
        }
        guard let h = try? FileHandle(forWritingTo: url) else {
            fatalError("Could not open CSV log at \(path)")
        }
        handle = h
        _ = try? handle.seekToEnd()
        if needsHeader {
            write(line: "iso,mode,press,cold_or_warm,mic,"
                + "engine_start_ms,first_sample_ms_since_t0,tone_play_call_ms_since_t0,"
                + "tap_host_ms_since_t0,sample_rate,channels")
        }
    }

    func write(line: String) {
        guard let data = (line + "\n").data(using: .utf8) else { return }
        handle.write(data)
    }

    var path: String { url.path }
}

// MARK: - Harness

struct PressMetric {
    let press: Int
    let engineStartMs: Double
    let firstSampleMs: Double
    let tonePlayCallMs: Double
    let tapHostMs: Double
}

final class Harness {
    enum State { case idle, recording }

    private let mode: WarmMode
    private let csv: CSVLog
    private let toneBuffer: AVAudioPCMBuffer

    private var state: State = .idle
    private var pressCount: Int = 0
    private(set) var metrics: [PressMetric] = []

    // Engine state. In .warm, `engine` is built once and reused; the player
    // node stays attached. In .cold, we rebuild everything per press.
    private var engine: AVAudioEngine?
    private var playerNode: AVAudioPlayerNode?
    private var recordFile: AVAudioFile?

    // Per-press timing state
    private var t0: UInt64 = 0
    private var engineStartMs: Double = 0
    private var firstSampleMs: Double = 0
    private var tonePlayCallMs: Double = 0
    private var tapHostMs: Double = 0
    private var firstSampleSeen: Bool = false
    private var tapSampleRate: Double = 0
    private var tapChannels: UInt32 = 0
    private var sampleFramesWritten: UInt64 = 0

    init(mode: WarmMode, csv: CSVLog) {
        self.mode = mode
        self.csv = csv
        self.toneBuffer = makeToneBuffer()
        if mode == .warm {
            // Build the engine *and start it* once at init. In steady-state
            // warm presses pay only tap-install cost, not engine.start().
            // This one initial start pays the same ~500ms cold cost as a
            // cold-mode press; the win is on presses 2..N.
            buildEngineIfNeeded()
            if let engine = engine {
                do {
                    try engine.start()
                    fputs("  [warm] engine prewarmed and running\n", stderr)
                } catch {
                    fputs("warm engine.start() failed: \(error)\n", stderr)
                }
            }
        }
    }

    func onHotKey() {
        switch state {
        case .idle: startPress()
        case .recording: stopPress()
        }
    }

    // MARK: - Cold/warm engine construction

    private func buildEngineIfNeeded() {
        if engine != nil { return }
        let newEngine = AVAudioEngine()
        let player = AVAudioPlayerNode()
        newEngine.attach(player)
        newEngine.connect(player, to: newEngine.mainMixerNode, format: toneBuffer.format)
        // Touch inputNode so the engine knows we want input; AVAudioEngine
        // lazily configures the input bus on first access.
        _ = newEngine.inputNode
        engine = newEngine
        playerNode = player
    }

    // MARK: - Press lifecycle

    private func startPress() {
        t0 = nowTicks()
        pressCount += 1
        firstSampleSeen = false
        engineStartMs = 0
        firstSampleMs = 0
        tonePlayCallMs = 0
        tapHostMs = 0
        sampleFramesWritten = 0

        if mode == .cold {
            // Cold path: fully reconstruct the engine on every press. This is
            // what Handy currently does (minus lazy_stream_close).
            engine?.stop()
            engine = nil
            playerNode = nil
            buildEngineIfNeeded()
        } else {
            // Warm path: engine was started once at init and is still running.
            // We never call engine.stop() between presses.
            buildEngineIfNeeded()
        }

        guard let engine = engine, let player = playerNode else { return }

        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        tapSampleRate = format.sampleRate
        tapChannels = format.channelCount

        // Prepare recording file with a predictable name
        let fm = FileManager.default
        let dir = URL(fileURLWithPath: "/tmp/handy-audio-perf")
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let wavURL = dir.appendingPathComponent("press-\(mode.rawValue)-\(pressCount).wav")
        do {
            recordFile = try AVAudioFile(forWriting: wavURL, settings: format.settings)
        } catch {
            fputs("AVAudioFile open failed: \(error)\n", stderr)
            recordFile = nil
        }

        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buf, when in
            guard let self = self else { return }
            try? self.recordFile?.write(from: buf)
            self.sampleFramesWritten &+= UInt64(buf.frameLength)

            if !self.firstSampleSeen {
                self.firstSampleSeen = true
                let tSwift = nowTicks()
                // hostTime is in mach ticks — same units as mach_absolute_time()
                // and our t0. We use signed subtraction because in warm mode the
                // first delivered buffer can contain samples captured *before*
                // the keypress (engine was already running and buffering).
                self.tapHostMs = msBetweenSigned(self.t0, when.hostTime)
                self.firstSampleMs = msBetween(self.t0, tSwift)

                // Play the tone immediately.
                player.scheduleBuffer(self.toneBuffer, at: nil, options: [], completionHandler: nil)
                if !player.isPlaying {
                    player.play()
                }
                self.tonePlayCallMs = msSince(self.t0)
            }
        }

        switch mode {
        case .cold:
            // Full cold-open cost lives here.
            let tEngineStartPre = nowTicks()
            do {
                try engine.start()
            } catch {
                fputs("engine.start() failed: \(error)\n", stderr)
                input.removeTap(onBus: 0)
                return
            }
            engineStartMs = msSince(tEngineStartPre)
        case .warm:
            // Engine is already running — no start() cost on press. The
            // tap-install above is the only per-press engine work.
            engineStartMs = 0
        }

        state = .recording
        print(String(format: "[press %d %@] t0 ready, engine_start=%.2fms",
                     pressCount, mode.rawValue, engineStartMs))
    }

    private func stopPress() {
        guard let engine = engine else { return }
        engine.inputNode.removeTap(onBus: 0)
        recordFile = nil  // AVAudioFile flushes on dealloc

        // Log one CSV row per completed press
        let iso = ISO8601DateFormatter().string(from: Date())
        let mic = currentInputDeviceName() ?? "unknown"
        let row = [
            iso,
            mode.rawValue,
            String(pressCount),
            mode == .cold ? "cold" : "warm",
            mic,
            String(format: "%.3f", engineStartMs),
            String(format: "%.3f", firstSampleMs),
            String(format: "%.3f", tonePlayCallMs),
            String(format: "%.3f", tapHostMs),
            String(format: "%.0f", tapSampleRate),
            String(tapChannels),
        ].joined(separator: ",")
        csv.write(line: row)

        metrics.append(PressMetric(
            press: pressCount,
            engineStartMs: engineStartMs,
            firstSampleMs: firstSampleMs,
            tonePlayCallMs: tonePlayCallMs,
            tapHostMs: tapHostMs
        ))

        print(String(format:
            "[press %d %@] first_sample_swift=%.2fms first_sample_host=%.2fms "
            + "tone_play_call=%.2fms engine_start=%.2fms samples_written=%llu (logged)",
            pressCount, mode.rawValue,
            firstSampleMs, tapHostMs, tonePlayCallMs, engineStartMs,
            sampleFramesWritten))

        if mode == .cold {
            // Tear down only in cold mode. Warm mode keeps the engine alive.
            engine.stop()
            self.engine = nil
            self.playerNode = nil
        }
        state = .idle
    }
}

// MARK: - Input device name (for CSV context)

func currentInputDeviceName() -> String? {
    // Fetch the current default input device name via CoreAudio. Used only
    // for CSV annotation, never on the hot measurement path.
    var devID = AudioDeviceID(0)
    var size = UInt32(MemoryLayout.size(ofValue: devID))
    var addr = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    let status = AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject),
                                            &addr, 0, nil, &size, &devID)
    guard status == noErr else { return nil }

    var name: Unmanaged<CFString>?
    var nameSize = UInt32(MemoryLayout<Unmanaged<CFString>>.size)
    var nameAddr = AudioObjectPropertyAddress(
        mSelector: kAudioObjectPropertyName,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    let nstatus = AudioObjectGetPropertyData(devID, &nameAddr, 0, nil, &nameSize, &name)
    guard nstatus == noErr, let cfName = name?.takeRetainedValue() else { return nil }
    return cfName as String
}

// MARK: - Carbon hotkey registration

final class HotKey {
    private var hotKeyRef: EventHotKeyRef?
    // fileprivate so the C callback below (outside the class) can dispatch.
    fileprivate static var handlers: [UInt32: () -> Void] = [:]
    private static var installed = false
    private static var nextId: UInt32 = 1
    private let id: UInt32
    let label: String

    init(label: String, keyCode: UInt32, modifiers: UInt32, handler: @escaping () -> Void) {
        HotKey.installHandlerOnce()
        self.id = HotKey.nextId
        self.label = label
        HotKey.nextId += 1
        HotKey.handlers[self.id] = handler

        let hkID = EventHotKeyID(signature: OSType(0x50_45_52_46), id: self.id)  // 'PERF'
        let status = RegisterEventHotKey(keyCode, modifiers, hkID,
                                         GetApplicationEventTarget(), 0, &hotKeyRef)
        if status == noErr {
            fputs("  [OK]  registered hotkey '\(label)'\n", stderr)
        } else {
            // -9878 is eventHotKeyExistsErr — someone else has the combo.
            fputs("  [FAIL] hotkey '\(label)' could not register (OSStatus=\(status))\n", stderr)
            HotKey.handlers.removeValue(forKey: self.id)
        }
    }

    deinit {
        if let ref = hotKeyRef { UnregisterEventHotKey(ref) }
    }

    private static func installHandlerOnce() {
        if installed { return }
        installed = true
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard),
                                 eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(),
                            hotKeyCallback,
                            1, &spec, nil, nil)
    }
}

private let hotKeyCallback: EventHandlerUPP = { _, eventRef, _ -> OSStatus in
    var hkID = EventHotKeyID()
    let err = GetEventParameter(eventRef,
                                EventParamName(kEventParamDirectObject),
                                EventParamType(typeEventHotKeyID),
                                nil,
                                MemoryLayout<EventHotKeyID>.size,
                                nil, &hkID)
    if err == noErr, let handler = HotKey.handlers[hkID.id] {
        handler()
    }
    return noErr
}

// MARK: - Auto driver

/// Drives a sequence of press-on/press-off cycles against the Harness without
/// any human input. Used to verify the harness end-to-end and to collect
/// repeatable measurements for cold vs warm comparison.
final class AutoDriver {
    private let config: AutoConfig
    private let trigger: () -> Void
    private let onComplete: () -> Void
    private var current: Int = 0

    init(config: AutoConfig, trigger: @escaping () -> Void, onComplete: @escaping () -> Void) {
        self.config = config
        self.trigger = trigger
        self.onComplete = onComplete
    }

    func begin(initialDelay: Double = 0.5) {
        DispatchQueue.main.asyncAfter(deadline: .now() + initialDelay) { [weak self] in
            self?.startPress()
        }
    }

    private func startPress() {
        current += 1
        if current > config.iterations {
            onComplete()
            return
        }
        let hold = Double.random(in: config.minHold...config.maxHold)
        fputs(String(format:
            "  [auto] iteration %d/%d: starting press, will hold %.2fs\n",
            current, config.iterations, hold), stderr)
        trigger()
        DispatchQueue.main.asyncAfter(deadline: .now() + hold) { [weak self] in
            self?.stopPress()
        }
    }

    private func stopPress() {
        trigger()
        DispatchQueue.main.asyncAfter(deadline: .now() + config.idle) { [weak self] in
            self?.startPress()
        }
    }
}

// MARK: - Summary statistics

struct Stats {
    let min: Double
    let median: Double
    let mean: Double
    let max: Double

    static func from(_ xs: [Double]) -> Stats? {
        guard !xs.isEmpty else { return nil }
        let sorted = xs.sorted()
        let mean = xs.reduce(0, +) / Double(xs.count)
        let median: Double
        if sorted.count.isMultiple(of: 2) {
            median = (sorted[sorted.count / 2 - 1] + sorted[sorted.count / 2]) / 2
        } else {
            median = sorted[sorted.count / 2]
        }
        return Stats(min: sorted.first!, median: median, mean: mean, max: sorted.last!)
    }
}

/// Pad a string to a fixed width (left-justified, space-padded). Avoids the
/// `printf %s` + Swift-String gotcha.
@inline(__always)
func pad(_ s: String, _ width: Int, rightAlign: Bool = false) -> String {
    if s.count >= width { return s }
    let spaces = String(repeating: " ", count: width - s.count)
    return rightAlign ? spaces + s : s + spaces
}

@inline(__always)
func num(_ x: Double, width: Int) -> String {
    pad(String(format: "%.2f", x), width, rightAlign: true)
}

func printSummary(mode: WarmMode, metrics: [PressMetric]) {
    guard !metrics.isEmpty else { return }
    print("")
    print("==== summary (\(metrics.count) presses, mode=\(mode.rawValue)) ====")
    let columns: [(String, KeyPath<PressMetric, Double>)] = [
        ("engine_start_ms",   \.engineStartMs),
        ("first_sample_ms",   \.firstSampleMs),
        ("tone_play_call_ms", \.tonePlayCallMs),
        ("tap_host_ms",       \.tapHostMs),
    ]
    print("  "
        + pad("metric", 22) + "  "
        + pad("min", 10, rightAlign: true) + "  "
        + pad("median", 10, rightAlign: true) + "  "
        + pad("mean", 10, rightAlign: true) + "  "
        + pad("max", 10, rightAlign: true))
    for (name, kp) in columns {
        let xs = metrics.map { $0[keyPath: kp] }
        guard let s = Stats.from(xs) else { continue }
        print("  "
            + pad(name, 22) + "  "
            + num(s.min,    width: 10) + "  "
            + num(s.median, width: 10) + "  "
            + num(s.mean,   width: 10) + "  "
            + num(s.max,    width: 10))
    }
    // Per-press table to make outliers obvious (press 1 in warm mode should
    // stand out as slower than the rest).
    print("\n  per-press:")
    print("    "
        + pad("press", 8) + "  "
        + pad("engine_start", 14, rightAlign: true) + "  "
        + pad("first_sample", 14, rightAlign: true) + "  "
        + pad("tone_play",    14, rightAlign: true) + "  "
        + pad("tap_host",     14, rightAlign: true))
    for m in metrics {
        print("    "
            + pad(String(m.press), 8) + "  "
            + num(m.engineStartMs,   width: 14) + "  "
            + num(m.firstSampleMs,   width: 14) + "  "
            + num(m.tonePlayCallMs,  width: 14) + "  "
            + num(m.tapHostMs,       width: 14))
    }
    print("")
}

// MARK: - Stdin trigger (fallback when a hotkey isn't reaching us)

/// Read from stdin on a background thread; every line triggers a press toggle.
/// Useful for sanity-checking the harness inside a terminal even if no global
/// hotkey survives macOS event routing.
func installStdinTrigger(onLine: @escaping () -> Void) {
    DispatchQueue.global(qos: .userInitiated).async {
        while let line = readLine(strippingNewline: true) {
            _ = line  // content doesn't matter; any Enter fires a press
            // Dispatch back to main so we touch AVAudioEngine from the main thread.
            DispatchQueue.main.async { onLine() }
        }
    }
}

// MARK: - Entry point

final class AppDelegate: NSObject, NSApplicationDelegate {
    var harness: Harness!
    // Hold refs so they aren't deinit'd immediately.
    var hotKeys: [HotKey] = []
    var autoDriver: AutoDriver?
    var mode: WarmMode = .cold

    func applicationDidFinishLaunching(_: Notification) {
        let args = CommandLine.arguments
        mode = WarmMode.parse(args)
        let csvPath = args.first(where: { $0.hasPrefix("--csv=") })
            .map { String($0.dropFirst("--csv=".count)) }
            ?? "/tmp/handy-audio-perf/perf.csv"

        let fm = FileManager.default
        try? fm.createDirectory(atPath: (csvPath as NSString).deletingLastPathComponent,
                                withIntermediateDirectories: true)

        let csv = CSVLog(path: csvPath)
        harness = Harness(mode: mode, csv: csv)

        let toggle: () -> Void = { [weak self] in
            self?.harness.onHotKey()
        }

        print("""

        handy-audio-perf ready.
          mode: \(mode.rawValue)
          csv:  \(csv.path)
          mic:  \(currentInputDeviceName() ?? "unknown")
        """)

        // Auto mode drives the harness itself. Skip interactive hotkeys so
        // there's no human element in the measurement.
        if let auto = AutoConfig.parse(args) {
            print("""

              auto mode: \(auto.iterations) iterations, \
            hold \(String(format: "%.1f", auto.minHold))..\(String(format: "%.1f", auto.maxHold))s, \
            idle \(String(format: "%.1f", auto.idle))s

            """)
            autoDriver = AutoDriver(config: auto, trigger: toggle) { [weak self] in
                self?.finish()
            }
            autoDriver?.begin()
            return
        }

        print("\nRegistering hotkeys (any of these will toggle a press):")

        // Multiple candidate global hotkeys. macOS 15+ requires at least one
        // modifier that isn't shift/option, which is why every combo below
        // includes either Cmd or Ctrl. We register all of them so the user
        // has options if one conflicts with a system or app shortcut.
        let candidates: [(String, UInt32, UInt32)] = [
            ("Cmd+Shift+H",     UInt32(cmdKey | shiftKey),              UInt32(kVK_ANSI_H)),
            ("Ctrl+Shift+H",    UInt32(controlKey | shiftKey),          UInt32(kVK_ANSI_H)),
            ("Ctrl+Opt+Space",  UInt32(controlKey | optionKey),         UInt32(kVK_Space)),
            ("F19",             0,                                      UInt32(kVK_F19)),
        ]
        for (label, modifiers, keyCode) in candidates {
            hotKeys.append(HotKey(label: label, keyCode: keyCode, modifiers: modifiers, handler: toggle))
        }

        installStdinTrigger(onLine: toggle)

        print("""

        Fallback: press Enter in this terminal to toggle a press.
        Ctrl+C to exit.

        """)
    }

    /// Called from auto mode once all iterations finish. Prints a summary
    /// table then exits cleanly so the shell caller sees a normal exit.
    private func finish() {
        printSummary(mode: mode, metrics: harness.metrics)
        NSApp.terminate(nil)
    }
}

// Swift `print` fully buffers stdout when it's a pipe. For a harness that's
// meant to be watched live, we want lines to appear immediately.
setbuf(stdout, nil)
setbuf(stderr, nil)

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
