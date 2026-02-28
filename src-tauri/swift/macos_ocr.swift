import AppKit
import CoreGraphics
import Foundation
import Vision

public typealias OCRResponsePointer = UnsafeMutablePointer<OCRTextResponse>
private let MIN_WINDOW_DIMENSION: CGFloat = 32.0

private func duplicateCString(_ text: String) -> UnsafeMutablePointer<CChar>? {
    return text.withCString { basePointer in
        guard let duplicated = strdup(basePointer) else {
            return nil
        }
        return duplicated
    }
}

private func makeResponse(text: String) -> OCRResponsePointer {
    let responsePtr = OCRResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(to: OCRTextResponse(text: nil, success: 0, error_message: nil))
    responsePtr.pointee.success = 1
    responsePtr.pointee.text = duplicateCString(text)
    return responsePtr
}

private func makeErrorResponse(_ message: String) -> OCRResponsePointer {
    let responsePtr = OCRResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(to: OCRTextResponse(text: nil, success: 0, error_message: nil))
    responsePtr.pointee.error_message = duplicateCString(message)
    return responsePtr
}

private func preflightScreenCaptureAccess() -> Bool {
    guard #available(macOS 10.15, *) else {
        return true
    }
    return CGPreflightScreenCaptureAccess()
}

private func requestScreenCaptureAccess() -> Bool {
    guard #available(macOS 10.15, *) else {
        return true
    }
    return CGRequestScreenCaptureAccess()
}

private func frontmostApplicationPID() -> pid_t? {
    return NSWorkspace.shared.frontmostApplication?.processIdentifier
}

private func frontmostWindowID(for ownerPID: pid_t) -> CGWindowID? {
    guard
        let windowList = CGWindowListCopyWindowInfo(
            [.optionOnScreenOnly, .excludeDesktopElements],
            kCGNullWindowID
        ) as? [[String: Any]]
    else {
        return nil
    }

    for window in windowList {
        guard let windowPID = window[kCGWindowOwnerPID as String] as? pid_t else {
            continue
        }
        guard windowPID == ownerPID else {
            continue
        }

        let windowLayer = (window[kCGWindowLayer as String] as? Int) ?? 0
        guard windowLayer == 0 else {
            continue
        }

        let alpha = (window[kCGWindowAlpha as String] as? CGFloat) ?? 1.0
        guard alpha > 0 else {
            continue
        }

        guard let boundsDict = window[kCGWindowBounds as String] as? NSDictionary else {
            continue
        }
        guard let bounds = CGRect(dictionaryRepresentation: boundsDict) else {
            continue
        }
        guard bounds.width >= MIN_WINDOW_DIMENSION && bounds.height >= MIN_WINDOW_DIMENSION else {
            continue
        }

        guard let windowNumber = window[kCGWindowNumber as String] as? UInt32 else {
            continue
        }

        return CGWindowID(windowNumber)
    }

    return nil
}

private func captureWindowImage(windowID: CGWindowID) -> CGImage? {
    return CGWindowListCreateImage(
        CGRect.null,
        .optionIncludingWindow,
        windowID,
        [.bestResolution, .boundsIgnoreFraming]
    )
}

@available(macOS 10.15, *)
private func recognizeText(in image: CGImage) throws -> String {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true

    let handler = VNImageRequestHandler(cgImage: image, options: [:])
    try handler.perform([request])

    guard let observations = request.results else {
        return ""
    }

    let lines = observations.compactMap { observation -> String? in
        guard let bestCandidate = observation.topCandidates(1).first else {
            return nil
        }
        let text = bestCandidate.string.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }

    return lines.joined(separator: "\n")
}

@_cdecl("macos_ocr_preflight_screen_capture_access")
public func macosOCRPreflightScreenCaptureAccess() -> Int32 {
    return preflightScreenCaptureAccess() ? 1 : 0
}

@_cdecl("macos_ocr_request_screen_capture_access")
public func macosOCRRequestScreenCaptureAccess() -> Int32 {
    return requestScreenCaptureAccess() ? 1 : 0
}

@_cdecl("macos_ocr_capture_frontmost_window_text")
public func macosOCRCaptureFrontmostWindowText() -> OCRResponsePointer {
    guard #available(macOS 10.15, *) else {
        return makeErrorResponse("macOS OCR requires macOS 10.15 or newer.")
    }

    guard preflightScreenCaptureAccess() else {
        return makeErrorResponse("Screen recording permission is not granted.")
    }

    guard let pid = frontmostApplicationPID() else {
        return makeErrorResponse("Unable to determine the frontmost application.")
    }

    guard let windowID = frontmostWindowID(for: pid) else {
        return makeErrorResponse("Unable to find a capturable frontmost window.")
    }

    guard let image = captureWindowImage(windowID: windowID) else {
        return makeErrorResponse("Failed to capture the frontmost window image.")
    }

    do {
        let text = try recognizeText(in: image)
        return makeResponse(text: text)
    } catch {
        return makeErrorResponse("OCR text recognition failed: \(error.localizedDescription)")
    }
}

@_cdecl("macos_ocr_free_response")
public func macosOCRFreeResponse(_ response: OCRResponsePointer?) {
    guard let response = response else {
        return
    }

    if let text = response.pointee.text {
        free(UnsafeMutablePointer(mutating: text))
    }

    if let errorMessage = response.pointee.error_message {
        free(UnsafeMutablePointer(mutating: errorMessage))
    }

    response.deallocate()
}
