import AudioToolbox
import CoreAudio
import Foundation

// System-wide audio ducking via Core Audio process taps (macOS 14.2+).
//
// While ducked, a muted process tap captures the output of every process
// except Handy itself (so recording feedback sounds stay at full volume),
// silencing their direct output. The tapped audio is replayed at a reduced
// gain through an aggregate device that wraps the current default output
// device — same clock, so no drift compensation or ring buffers are needed.
// The audio is only gain-scaled and replayed in the IO callback; nothing is
// stored or inspected. Tearing the tap down restores the original audio
// instantly.
//
// This works regardless of whether the output device exposes a software
// volume control (USB interfaces and HDMI outputs usually don't), and covers
// every audio source: browsers, music players, games, etc.

/// Tiny lock-guarded flag the realtime IO callback can set and the control
/// thread can poll. The lock is effectively uncontended (one writer, rare
/// polls), so taking it in the IO callback is pragmatic.
private final class SignalFlag {
    private var value = false
    private let lock = NSLock()

    func set() {
        lock.lock()
        value = true
        lock.unlock()
    }

    func get() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return value
    }

    func reset() {
        lock.lock()
        value = false
        lock.unlock()
    }
}

@available(macOS 14.2, *)
private final class AudioDucker {
    static let shared = AudioDucker()

    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var aggregateID = AudioObjectID(kAudioObjectUnknown)
    private var ioProcID: AudioDeviceIOProcID?
    private var running = false
    private let lock = NSLock()
    private let queue = DispatchQueue(label: "com.pais.handy.audio-duck")

    /// Set by the IO callback once it sees nonzero audio in the tap buffer.
    /// Without the system audio recording permission the tap is created
    /// successfully but stays inert (all-zero buffers, no muting), so callers
    /// must poll this after starting to confirm the duck actually engaged.
    let signalDetected = SignalFlag()

    private func systemObjectID() -> AudioObjectID {
        AudioObjectID(kAudioObjectSystemObject)
    }

    private func globalAddress(_ selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
    }

    private func defaultOutputDevice() -> AudioObjectID? {
        var address = globalAddress(kAudioHardwarePropertyDefaultOutputDevice)
        var deviceID = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let status = AudioObjectGetPropertyData(systemObjectID(), &address, 0, nil, &size, &deviceID)
        guard status == noErr, deviceID != kAudioObjectUnknown else { return nil }
        return deviceID
    }

    private func deviceUID(_ deviceID: AudioObjectID) -> String? {
        var address = globalAddress(kAudioDevicePropertyDeviceUID)
        var uid: CFString = "" as CFString
        var size = UInt32(MemoryLayout<CFString>.size)
        let status = withUnsafeMutablePointer(to: &uid) { ptr in
            AudioObjectGetPropertyData(deviceID, &address, 0, nil, &size, ptr)
        }
        guard status == noErr else { return nil }
        return uid as String
    }

    /// Number of input buffers the device contributes to an aggregate's input
    /// AudioBufferList. Tap streams are appended after sub-device streams, so
    /// this is the index of the first tap buffer in the IO callback.
    private func inputBufferCount(of deviceID: AudioObjectID) -> Int {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(deviceID, &address, 0, nil, &size) == noErr,
            size > 0
        else { return 0 }
        let raw = UnsafeMutableRawPointer.allocate(
            byteCount: Int(size), alignment: MemoryLayout<AudioBufferList>.alignment)
        defer { raw.deallocate() }
        guard AudioObjectGetPropertyData(deviceID, &address, 0, nil, &size, raw) == noErr else {
            return 0
        }
        return Int(raw.assumingMemoryBound(to: AudioBufferList.self).pointee.mNumberBuffers)
    }

    private func ownProcessObject() -> AudioObjectID? {
        var pid = pid_t(ProcessInfo.processInfo.processIdentifier)
        var address = globalAddress(kAudioHardwarePropertyTranslatePIDToProcessObject)
        var processObject = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let status = withUnsafeMutablePointer(to: &pid) { pidPtr in
            AudioObjectGetPropertyData(
                systemObjectID(), &address,
                UInt32(MemoryLayout<pid_t>.size), pidPtr,
                &size, &processObject)
        }
        guard status == noErr, processObject != kAudioObjectUnknown else { return nil }
        return processObject
    }

    func start(gain: Float) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if running { return true }

        // Excluding our own process is mandatory: without it the replayed
        // audio would be tapped again, producing a feedback loop.
        guard let ownProcess = ownProcessObject() else {
            NSLog("[audio_duck] could not translate own PID to process object")
            return false
        }
        guard let outputDevice = defaultOutputDevice(),
            let outputUID = deviceUID(outputDevice)
        else {
            NSLog("[audio_duck] no default output device")
            return false
        }

        let description = CATapDescription(
            stereoGlobalTapButExcludeProcesses: [ownProcess])
        description.name = "Handy Duck Tap"
        description.isPrivate = true
        // Tapped processes are silenced only while our IO proc is running, so
        // audio comes back by itself even if Handy dies without cleaning up.
        description.muteBehavior = CATapMuteBehavior.mutedWhenTapped

        var newTapID = AudioObjectID(kAudioObjectUnknown)
        var status = AudioHardwareCreateProcessTap(description, &newTapID)
        guard status == noErr, newTapID != kAudioObjectUnknown else {
            // Most commonly: system audio recording permission not granted
            // (the prompt appears on the first attempt).
            NSLog("[audio_duck] AudioHardwareCreateProcessTap failed: \(status)")
            return false
        }
        tapID = newTapID

        let aggregateDescription: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Handy Duck",
            kAudioAggregateDeviceUIDKey: "com.pais.handy.duck." + UUID().uuidString,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [
                [kAudioSubDeviceUIDKey: outputUID]
            ],
            kAudioAggregateDeviceTapListKey: [
                [
                    kAudioSubTapUIDKey: description.uuid.uuidString,
                    kAudioSubTapDriftCompensationKey: true,
                ]
            ],
            kAudioAggregateDeviceTapAutoStartKey: true,
        ]

        var newAggregateID = AudioObjectID(kAudioObjectUnknown)
        status = AudioHardwareCreateAggregateDevice(
            aggregateDescription as CFDictionary, &newAggregateID)
        guard status == noErr, newAggregateID != kAudioObjectUnknown else {
            NSLog("[audio_duck] AudioHardwareCreateAggregateDevice failed: \(status)")
            teardownLocked()
            return false
        }
        aggregateID = newAggregateID

        // The output device's own input streams (e.g. a USB interface's line
        // inputs) come first in the input buffer list; the tap follows them.
        let tapBufferIndex = inputBufferCount(of: outputDevice)
        let duckGain = gain
        signalDetected.reset()
        let signalFlag = signalDetected

        var newProcID: AudioDeviceIOProcID?
        status = AudioDeviceCreateIOProcIDWithBlock(&newProcID, aggregateID, queue) {
            _, inInputData, _, outOutputData, _ in
            let inputList = UnsafeMutableAudioBufferListPointer(
                UnsafeMutablePointer(mutating: inInputData))
            let outputList = UnsafeMutableAudioBufferListPointer(outOutputData)

            for buffer in outputList {
                if let data = buffer.mData {
                    memset(data, 0, Int(buffer.mDataByteSize))
                }
            }

            // Prefer the computed tap index; fall back to the last input
            // buffer (the tap is always appended after device streams).
            var index = tapBufferIndex
            if index >= inputList.count {
                index = inputList.count - 1
            }
            guard index >= 0, index < inputList.count else { return }
            let input = inputList[index]
            guard let inData = input.mData?.assumingMemoryBound(to: Float32.self),
                let output = outputList.first(where: { $0.mNumberChannels > 0 }),
                let outData = output.mData?.assumingMemoryBound(to: Float32.self)
            else { return }

            let inChannels = Int(input.mNumberChannels)
            let outChannels = Int(output.mNumberChannels)
            guard inChannels > 0, outChannels > 0 else { return }
            let frames = min(
                Int(input.mDataByteSize) / (MemoryLayout<Float32>.size * inChannels),
                Int(output.mDataByteSize) / (MemoryLayout<Float32>.size * outChannels))
            let channels = min(inChannels, outChannels)
            var energy: Float = 0
            for frame in 0..<frames {
                for channel in 0..<channels {
                    let sample = inData[frame * inChannels + channel]
                    energy += abs(sample)
                    outData[frame * outChannels + channel] = sample * duckGain
                }
            }
            if energy > 0.001 {
                signalFlag.set()
            }
        }
        guard status == noErr, let procID = newProcID else {
            NSLog("[audio_duck] AudioDeviceCreateIOProcIDWithBlock failed: \(status)")
            teardownLocked()
            return false
        }
        ioProcID = procID

        status = AudioDeviceStart(aggregateID, procID)
        guard status == noErr else {
            NSLog("[audio_duck] AudioDeviceStart failed: \(status)")
            teardownLocked()
            return false
        }

        running = true
        return true
    }

    func stop() {
        lock.lock()
        defer { lock.unlock() }
        teardownLocked()
    }

    /// Creating a tap is what triggers the system audio recording permission
    /// prompt, so do it once (and immediately tear it down) at a convenient
    /// time instead of in the middle of the first dictation.
    func requestPermission() {
        lock.lock()
        defer { lock.unlock() }
        if running { return }
        guard let ownProcess = ownProcessObject() else { return }
        let description = CATapDescription(
            stereoGlobalTapButExcludeProcesses: [ownProcess])
        description.name = "Handy Duck Permission Check"
        description.isPrivate = true
        var probeTapID = AudioObjectID(kAudioObjectUnknown)
        if AudioHardwareCreateProcessTap(description, &probeTapID) == noErr,
            probeTapID != kAudioObjectUnknown
        {
            AudioHardwareDestroyProcessTap(probeTapID)
        }
    }

    private func teardownLocked() {
        if let procID = ioProcID {
            if running {
                AudioDeviceStop(aggregateID, procID)
            }
            AudioDeviceDestroyIOProcID(aggregateID, procID)
            ioProcID = nil
        }
        if aggregateID != kAudioObjectUnknown {
            AudioHardwareDestroyAggregateDevice(aggregateID)
            aggregateID = AudioObjectID(kAudioObjectUnknown)
        }
        if tapID != kAudioObjectUnknown {
            AudioHardwareDestroyProcessTap(tapID)
            tapID = AudioObjectID(kAudioObjectUnknown)
        }
        running = false
    }
}

@_cdecl("handy_audio_duck_supported")
public func handyAudioDuckSupported() -> Int32 {
    if #available(macOS 14.2, *) {
        return 1
    }
    return 0
}

@_cdecl("handy_audio_duck_start")
public func handyAudioDuckStart(_ gain: Float) -> Int32 {
    if #available(macOS 14.2, *) {
        return AudioDucker.shared.start(gain: gain) ? 1 : 0
    }
    return 0
}

@_cdecl("handy_audio_duck_stop")
public func handyAudioDuckStop() {
    if #available(macOS 14.2, *) {
        AudioDucker.shared.stop()
    }
}

/// Returns 1 once the running tap has seen nonzero audio. A tap created
/// without the system audio recording permission stays inert (silence, no
/// muting), which is indistinguishable from "nothing is playing" — callers
/// should poll this briefly after start and fall back to other ducking
/// strategies when it stays 0.
@_cdecl("handy_audio_duck_has_signal")
public func handyAudioDuckHasSignal() -> Int32 {
    if #available(macOS 14.2, *) {
        return AudioDucker.shared.signalDetected.get() ? 1 : 0
    }
    return 0
}

@_cdecl("handy_audio_duck_request_permission")
public func handyAudioDuckRequestPermission() {
    if #available(macOS 14.2, *) {
        AudioDucker.shared.requestPermission()
    }
}
