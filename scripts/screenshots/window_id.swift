// Prints the window IDs of a process's on-screen windows, largest first, for
// `screencapture -l <ID>` (spec/development.md, "Store screenshots"):
//
//   swift scripts/screenshots/window_id.swift PID
//   screencapture -o -l <ID> shot.png
//
// Each line is "ID WIDTHxHEIGHT" in points. Window titles are not read, so
// this needs no Screen Recording permission (screencapture itself does).

import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 2, let pid = Int(CommandLine.arguments[1]) else {
    FileHandle.standardError.write("usage: window_id.swift PID\n".data(using: .utf8)!)
    exit(2)
}

let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    as? [[String: Any]] ?? []
var found: [(id: Int, rect: CGRect)] = []
for window in info {
    guard window[kCGWindowOwnerPID as String] as? Int == pid,
          window[kCGWindowLayer as String] as? Int == 0,
          let id = window[kCGWindowNumber as String] as? Int,
          let dict = window[kCGWindowBounds as String] as? NSDictionary,
          let rect = CGRect(dictionaryRepresentation: dict as CFDictionary)
    else { continue }
    found.append((id, rect))
}
if found.isEmpty {
    FileHandle.standardError.write("no on-screen window of process \(pid)\n".data(using: .utf8)!)
    exit(1)
}
for window in found.sorted(by: { $0.rect.width * $0.rect.height > $1.rect.width * $1.rect.height }) {
    print("\(window.id) \(Int(window.rect.width))x\(Int(window.rect.height))")
}
