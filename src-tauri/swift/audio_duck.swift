import Foundation
import CoreAudio

// Minimal Core Audio Process Tap ducking, based on WisprDuck (MIT license).
// Ducks all audio-producing processes by intercepting their output via
// AudioHardwareCreateProcessTap with mutedWhenTapped, scaling samples by
// duck_level, and routing through an aggregate device. Crash-safe: macOS
// auto-unmutes if this process dies.
//
// Requires macOS 14.2+ and "Screen & System Audio Recording" permission.

private struct ActiveTap {
    let pid: pid_t
    let tapID: AudioObjectID
    let aggregateID: AudioObjectID
    var ioProcID: AudioDeviceIOProcID?
    let tapUUID: UUID
    let aggregateUUID: UUID
}

private var activeTaps: [pid_t: ActiveTap] = [:]
private var currentDuckLevel: Float = 0.3
private let duckQueue = DispatchQueue(label: "com.handy.audioduck", qos: .userInitiated)

// MARK: - Public C API

@_cdecl("audio_duck_start")
public func audioDuckStart(duckLevel: Float) -> Int32 {
    guard #available(macOS 14.2, *) else { return -1 }

    let level = max(0.0, min(1.0, duckLevel))
    currentDuckLevel = level

    guard let outputUID = getDefaultOutputDeviceUID() else { return -2 }

    let processes = enumerateAudioProcesses()
    if processes.isEmpty { return 0 }

    var started = 0
    for proc in processes {
        if activeTaps[proc.pid] != nil { continue }
        if startTap(for: proc, outputUID: outputUID, duckLevel: level) {
            started += 1
        }
    }

    return Int32(started)
}

@_cdecl("audio_duck_stop")
public func audioDuckStop() -> Int32 {
    for (pid, tap) in activeTaps {
        stopTap(tap)
        activeTaps.removeValue(forKey: pid)
    }
    return 0
}

@_cdecl("audio_duck_is_active")
public func audioDuckIsActive() -> Int32 {
    return activeTaps.isEmpty ? 0 : 1
}

// MARK: - Process Enumeration

private struct AudioProc {
    let pid: pid_t
    let objectID: AudioObjectID
}

private func enumerateAudioProcesses() -> [AudioProc] {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyProcessObjectList,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var size: UInt32 = 0
    var status = AudioObjectGetPropertyDataSize(
        AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size
    )
    guard status == noErr, size > 0 else { return [] }

    let count = Int(size) / MemoryLayout<AudioObjectID>.size
    var objectIDs = [AudioObjectID](repeating: 0, count: count)
    status = AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &objectIDs
    )
    guard status == noErr else { return [] }

    let myPID = ProcessInfo.processInfo.processIdentifier
    return objectIDs.compactMap { objectID -> AudioProc? in
        guard let pid = pidFor(objectID), pid != myPID else { return nil }
        return AudioProc(pid: pid, objectID: objectID)
    }
}

private func pidFor(_ objectID: AudioObjectID) -> pid_t? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioProcessPropertyPID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var pid: pid_t = 0
    var size = UInt32(MemoryLayout<pid_t>.size)
    let status = AudioObjectGetPropertyData(objectID, &address, 0, nil, &size, &pid)
    guard status == noErr, pid > 0 else { return nil }
    return pid
}

// MARK: - Tap Lifecycle

@available(macOS 14.2, *)
private func startTap(for proc: AudioProc, outputUID: String, duckLevel: Float) -> Bool {
    let tapUUID = UUID()
    let aggUUID = UUID()

    let tapDesc = CATapDescription(stereoMixdownOfProcesses: [proc.objectID])
    tapDesc.uuid = tapUUID
    tapDesc.name = "HandyDuck-\(proc.pid)"
    tapDesc.muteBehavior = .mutedWhenTapped
    tapDesc.isPrivate = true

    var tapID: AudioObjectID = kAudioObjectUnknown
    var status = AudioHardwareCreateProcessTap(tapDesc, &tapID)
    guard status == noErr else { return false }

    let aggDesc: [String: Any] = [
        kAudioAggregateDeviceNameKey: "HandyDuck-Agg-\(proc.pid)",
        kAudioAggregateDeviceUIDKey: aggUUID.uuidString,
        kAudioAggregateDeviceMainSubDeviceKey: outputUID,
        kAudioAggregateDeviceClockDeviceKey: outputUID,
        kAudioAggregateDeviceIsPrivateKey: true,
        kAudioAggregateDeviceIsStackedKey: false,
        kAudioAggregateDeviceTapAutoStartKey: true,
        kAudioAggregateDeviceSubDeviceListKey: [
            [kAudioSubDeviceUIDKey: outputUID, kAudioSubDeviceDriftCompensationKey: false]
        ],
        kAudioAggregateDeviceTapListKey: [
            [kAudioSubTapUIDKey: tapUUID.uuidString, kAudioSubTapDriftCompensationKey: true]
        ],
    ]

    var aggID: AudioObjectID = kAudioObjectUnknown
    status = AudioHardwareCreateAggregateDevice(aggDesc as CFDictionary, &aggID)
    guard status == noErr else {
        AudioHardwareDestroyProcessTap(tapID)
        return false
    }

    var ioProcID: AudioDeviceIOProcID?
    let level = duckLevel
    status = AudioDeviceCreateIOProcIDWithBlock(&ioProcID, aggID, duckQueue) {
        _, inInputData, _, outOutputData, _ in
        let inputs = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inInputData))
        let outputs = UnsafeMutableAudioBufferListPointer(outOutputData)
        let tapOffset = max(0, inputs.count - outputs.count)

        for (i, output) in outputs.enumerated() {
            let inputIndex = tapOffset + i
            guard inputIndex < inputs.count,
                  let inData = inputs[inputIndex].mData,
                  let outData = output.mData else {
                if let outData = output.mData {
                    memset(outData, 0, Int(output.mDataByteSize))
                }
                continue
            }
            let inSamples = inData.assumingMemoryBound(to: Float.self)
            let outSamples = outData.assumingMemoryBound(to: Float.self)
            let byteCount = min(Int(inputs[inputIndex].mDataByteSize), Int(output.mDataByteSize))
            let sampleCount = byteCount / MemoryLayout<Float>.size

            for j in 0..<sampleCount {
                outSamples[j] = inSamples[j] * level
            }
        }
    }
    guard status == noErr else {
        AudioHardwareDestroyAggregateDevice(aggID)
        AudioHardwareDestroyProcessTap(tapID)
        return false
    }

    status = AudioDeviceStart(aggID, ioProcID)
    guard status == noErr else {
        if let procID = ioProcID { AudioDeviceDestroyIOProcID(aggID, procID) }
        AudioHardwareDestroyAggregateDevice(aggID)
        AudioHardwareDestroyProcessTap(tapID)
        return false
    }

    activeTaps[proc.pid] = ActiveTap(
        pid: proc.pid, tapID: tapID, aggregateID: aggID,
        ioProcID: ioProcID, tapUUID: tapUUID, aggregateUUID: aggUUID
    )
    return true
}

private func stopTap(_ tap: ActiveTap) {
    if let procID = tap.ioProcID {
        AudioDeviceStop(tap.aggregateID, procID)
        AudioDeviceDestroyIOProcID(tap.aggregateID, procID)
    }
    AudioHardwareDestroyAggregateDevice(tap.aggregateID)
    AudioHardwareDestroyProcessTap(tap.tapID)
}

// MARK: - Output Device

private func getDefaultOutputDeviceUID() -> String? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var deviceID: AudioDeviceID = kAudioObjectUnknown
    var size = UInt32(MemoryLayout<AudioDeviceID>.size)
    var status = AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &deviceID
    )
    guard status == noErr, deviceID != kAudioObjectUnknown else { return nil }

    var uidAddress = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyDeviceUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var uid: Unmanaged<CFString>?
    var uidSize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    status = AudioObjectGetPropertyData(deviceID, &uidAddress, 0, nil, &uidSize, &uid)
    guard status == noErr, let uid else { return nil }
    return uid.takeRetainedValue() as String
}
