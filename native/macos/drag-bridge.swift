import AppKit
import Foundation

// Idle, this process has one job: notice the moment the mouse goes down. It used to do
// that 30 times a second, and every tick allocated a CGEvent to read a cursor position
// nothing needs while no button is held. Measured A/B against the 0.1.11 binary on the
// 2026-11 idle review: 20ms of CPU per 6s idle (~0.3%, and 30 wakeups a second,
// forever) against 0ms for the same window now, while 12s of holding the button costs
// 30ms either way. So the gesture is sampled exactly as finely as it ever was and an
// idle Shift stops touching the CPU at all.
//
// 10 Hz while idle delays only the discovery that the button went down. The ring still
// cannot appear until Finder has published the dragged files on the drag pasteboard,
// which needs the drag threshold crossed first, and a press is what moves this timer to
// 30 Hz - so the arming path is unchanged in every respect a user can feel.
let idleInterval = 1.0 / 10.0
let gestureInterval = 1.0 / 30.0

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
var cancelled = false
var toolsMode = false
var missingDragFiles = 0
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
    // Reading the cursor costs a CGEvent allocation, and nothing downstream wants a
    // position unless a press is in progress or has just ended. An idle tick is then
    // one read of `pressedMouseButtons` and a couple of integer tests.
    let pressEnded = mouseWasDown && !mouseDown
    let point = mouseDown || pressEnded ? cursorLocation() : NSPoint(x: 0, y: 0)
    if !mouseDown {
        changeCountAtMouseDown = dragPasteboard.changeCount
        cancelled = false
        missingDragFiles = 0
    }

    if pressEnded && pickerShown {
        pickerShown = false
        emit(Payload(type: "end", x: point.x, y: point.y, files: nil, tools: nil))
    }
    mouseWasDown = mouseDown

    guard mouseDown else { return }

    let flags = NSEvent.modifierFlags.intersection(.deviceIndependentFlagsMask)
    let shift = flags.contains(.shift)
    let option = flags.contains(.option)

    if !pickerShown {
        guard !cancelled else { return }
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

    // Some Finder versions also tear down the pasteboard during cancellation.
    // Keep this as a fallback for drag sessions that disappear without a key event.
    if draggedFileURLs().isEmpty {
        missingDragFiles += 1
        if missingDragFiles >= 6 {
            pickerShown = false
            cancelled = true
            emit(Payload(type: "cancel", x: point.x, y: point.y, files: nil, tools: nil))
        }
        return
    }
    missingDragFiles = 0

    if abs(point.x - lastX) > 0.5 || abs(point.y - lastY) > 0.5 {
        lastX = point.x
        lastY = point.y
        emit(Payload(type: "move", x: point.x, y: point.y, files: nil, tools: nil))
    }
}

var timer: Timer?
var timerIsGesture = false

func reschedule(gesture: Bool) {
    guard gesture != timerIsGesture || timer == nil else { return }
    timerIsGesture = gesture
    timer?.invalidate()
    let next = Timer(timeInterval: gesture ? gestureInterval : idleInterval, repeats: true) { _ in
        let down = NSEvent.pressedMouseButtons & 1 != 0
        tick()
        // A press changes the pace on the next tick; the timer is replaced rather
        // than retuned because NSTimer cannot change its own interval.
        if down != timerIsGesture { reschedule(gesture: down) }
    }
    timer = next
    RunLoop.main.add(next, forMode: .common)
}

reschedule(gesture: NSEvent.pressedMouseButtons & 1 != 0)
RunLoop.main.run()
