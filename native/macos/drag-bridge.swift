import AppKit
import Foundation

struct Payload: Encodable {
    let type: String
    let x: Double?
    let y: Double?
    let files: [String]?
    let tools: Bool?
}

func emit(_ payload: Payload) {
    let encoder = JSONEncoder()
    guard let data = try? encoder.encode(payload),
          let line = String(data: data, encoding: .utf8) else { return }
    print(line)
    fflush(stdout)
}

func cursorLocation() -> NSPoint {
    if let event = CGEvent(source: nil) {
        return event.location
    }
    return NSEvent.mouseLocation
}

let dragPasteboard = NSPasteboard(name: .drag)
var mouseWasDown = false
var pickerShown = false
var toolsMode = false
var changeCountAtMouseDown = dragPasteboard.changeCount
var lastX = Double.nan
var lastY = Double.nan

func draggedFileURLs() -> [String] {
    let objects = dragPasteboard.readObjects(
        forClasses: [NSURL.self],
        options: [.urlReadingFileURLsOnly: true]
    )
    return (objects as? [URL])?.map(\.path) ?? []
}

func tick() {
    if getppid() == 1 {
        exit(0)
    }
    let mouseDown = NSEvent.pressedMouseButtons & 1 != 0
    let point = cursorLocation()

    if !mouseDown {
        changeCountAtMouseDown = dragPasteboard.changeCount
    }

    if !mouseDown && mouseWasDown && pickerShown {
        pickerShown = false
        emit(Payload(type: "end", x: point.x, y: point.y, files: nil, tools: nil))
    }
    mouseWasDown = mouseDown

    guard mouseDown else { return }

    let flags = NSEvent.modifierFlags.intersection(.deviceIndependentFlagsMask)
    let shift = flags.contains(.shift)
    let option = flags.contains(.option)

    if !pickerShown {
        guard shift, dragPasteboard.changeCount != changeCountAtMouseDown else { return }
        let files = draggedFileURLs()
        guard !files.isEmpty else { return }
        pickerShown = true
        toolsMode = option
        lastX = point.x
        lastY = point.y
        emit(Payload(type: "show", x: point.x, y: point.y, files: files, tools: option))
        return
    }

    if shift && option != toolsMode {
        toolsMode = option
        emit(Payload(type: "mode", x: point.x, y: point.y, files: nil, tools: option))
    }

    if abs(point.x - lastX) > 0.5 || abs(point.y - lastY) > 0.5 {
        lastX = point.x
        lastY = point.y
        emit(Payload(type: "move", x: point.x, y: point.y, files: nil, tools: nil))
    }
}

let timer = Timer(timeInterval: 1.0 / 30.0, repeats: true) { _ in
    tick()
}
RunLoop.main.add(timer, forMode: .common)
RunLoop.main.run()
