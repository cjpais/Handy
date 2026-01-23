import AVFoundation
import AudioToolbox
import CoreAudio
import Foundation

// MARK: - Swift implementation for audio feedback via system output device
// Uses AVAudioEngine routed to the system output device to avoid AirPods Handoff

/// Gets the UID of the system output device (used for alerts, doesn't trigger Handoff)
private func getSystemOutputDeviceUID() -> String? {
    var deviceID: AudioDeviceID = 0
    var propertySize = UInt32(MemoryLayout<AudioDeviceID>.size)

    var propertyAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultSystemOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    let status = AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject),
        &propertyAddress,
        0,
        nil,
        &propertySize,
        &deviceID
    )

    guard status == noErr, deviceID != kAudioDeviceUnknown else {
        return nil
    }

    // Get the UID string for this device
    var uidAddress = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyDeviceUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    var uid: Unmanaged<CFString>?
    var uidSize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)

    let uidStatus = AudioObjectGetPropertyData(
        deviceID,
        &uidAddress,
        0,
        nil,
        &uidSize,
        &uid
    )

    guard uidStatus == noErr, let unmanagedUID = uid else {
        return nil
    }

    return unmanagedUID.takeUnretainedValue() as String
}

/// Sets the output device for an AVAudioEngine
private func setOutputDevice(engine: AVAudioEngine, deviceUID: String) -> Bool {
    let outputNode = engine.outputNode
    let audioUnit = outputNode.audioUnit!

    var deviceID: AudioDeviceID = 0
    var propertySize = UInt32(MemoryLayout<AudioDeviceID>.size)

    // Translate UID to device ID
    var translationAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyTranslateUIDToDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    var uidCF: CFString = deviceUID as CFString
    let translationStatus = withUnsafeMutablePointer(to: &uidCF) { uidPtr in
        AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &translationAddress,
            UInt32(MemoryLayout<CFString>.size),
            uidPtr,
            &propertySize,
            &deviceID
        )
    }

    guard translationStatus == noErr, deviceID != kAudioDeviceUnknown else {
        return false
    }

    // Set the output device on the audio unit
    let setStatus = AudioUnitSetProperty(
        audioUnit,
        kAudioOutputUnitProperty_CurrentDevice,
        kAudioUnitScope_Global,
        0,
        &deviceID,
        UInt32(MemoryLayout<AudioDeviceID>.size)
    )

    return setStatus == noErr
}

/// Plays a sound file through the system output device with the specified volume
/// Returns: 0 on success, negative error code on failure
@_cdecl("play_sound_via_system_output")
public func playSoundViaSystemOutput(
    _ filePath: UnsafePointer<CChar>,
    _ volume: Float
) -> Int32 {
    let path = String(cString: filePath)
    let url = URL(fileURLWithPath: path)

    // Get the system output device UID
    guard let systemDeviceUID = getSystemOutputDeviceUID() else {
        return -1  // Failed to get system output device
    }

    // Load the audio file
    guard let audioFile = try? AVAudioFile(forReading: url) else {
        return -2  // Failed to load audio file
    }

    let engine = AVAudioEngine()
    let playerNode = AVAudioPlayerNode()

    engine.attach(playerNode)

    // Connect player to main mixer with the file's processing format
    let format = audioFile.processingFormat
    engine.connect(playerNode, to: engine.mainMixerNode, format: format)

    // Set the output device to system output (before starting the engine)
    guard setOutputDevice(engine: engine, deviceUID: systemDeviceUID) else {
        return -3  // Failed to set output device
    }

    // Set volume on the player node
    playerNode.volume = max(0.0, min(1.0, volume))

    // Prepare and start the engine
    engine.prepare()

    do {
        try engine.start()
    } catch {
        return -4  // Failed to start audio engine
    }

    // Read the entire file into a buffer
    let frameCount = AVAudioFrameCount(audioFile.length)
    guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frameCount) else {
        engine.stop()
        return -5  // Failed to create buffer
    }

    do {
        try audioFile.read(into: buffer)
    } catch {
        engine.stop()
        return -6  // Failed to read audio file
    }

    // Use a semaphore to wait for playback completion
    let semaphore = DispatchSemaphore(value: 0)

    playerNode.scheduleBuffer(buffer, at: nil, options: .interrupts) {
        semaphore.signal()
    }

    playerNode.play()

    // Calculate expected audio duration and use it as timeout.
    // This prevents 30-second hangs if the completion handler never fires
    // (e.g., if engine/player is stopped before playback completes).
    let durationSeconds = Double(buffer.frameLength) / format.sampleRate
    let timeoutSeconds = max(durationSeconds + 0.5, 1.0)  // At least 1 second, or duration + 0.5s buffer
    let timeout = DispatchTime.now() + .milliseconds(Int(timeoutSeconds * 1000))
    _ = semaphore.wait(timeout: timeout)

    // Cleanup
    playerNode.stop()
    engine.stop()

    return 0  // Success
}

/// Checks if the system output device is available
@_cdecl("is_system_output_available")
public func isSystemOutputAvailable() -> Int32 {
    return getSystemOutputDeviceUID() != nil ? 1 : 0
}
