import AppKit
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

guard CommandLine.arguments.count == 3 else {
    fail("usage: doc-render <input> <output.pdf>")
}

let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
let extensionName = input.pathExtension.lowercased()

var documentType: NSAttributedString.DocumentType?
switch extensionName {
case "docx":
    documentType = .officeOpenXML
case "doc":
    documentType = .docFormat
case "odt":
    documentType = .openDocument
case "rtf":
    documentType = .rtf
case "rtfd":
    documentType = .rtfd
case "html", "htm":
    documentType = .html
case "txt", "md", "markdown":
    documentType = .plain
default:
    documentType = nil
}

var options: [NSAttributedString.DocumentReadingOptionKey: Any] = [:]
if let documentType {
    options[.documentType] = documentType
}
if documentType == .html {
    options[.baseURL] = input.deletingLastPathComponent()
}

let data: Data
do {
    data = try Data(contentsOf: input)
} catch {
    fail("could not read input: \(error.localizedDescription)")
}

let attributed: NSAttributedString
do {
    attributed = try NSAttributedString(data: data, options: options, documentAttributes: nil)
} catch {
    fail("could not parse document: \(error.localizedDescription)")
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)

let printInfo = NSPrintInfo.shared.copy() as! NSPrintInfo
printInfo.paperSize = NSSize(width: 595.2, height: 841.8)
printInfo.topMargin = 42
printInfo.bottomMargin = 42
printInfo.leftMargin = 48
printInfo.rightMargin = 48
printInfo.horizontalPagination = .fit
printInfo.verticalPagination = .automatic
printInfo.isHorizontallyCentered = false
printInfo.isVerticallyCentered = false
printInfo.jobDisposition = .save
printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = output

let printable = printInfo.imageablePageBounds
let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: printable.width, height: 1))
textView.isEditable = false
textView.isSelectable = false
textView.drawsBackground = false
textView.textContainerInset = .zero
textView.textContainer?.lineFragmentPadding = 0
textView.textContainer?.widthTracksTextView = true
textView.textContainer?.containerSize = NSSize(
    width: printable.width,
    height: .greatestFiniteMagnitude
)
textView.textStorage?.setAttributedString(attributed)

guard let layoutManager = textView.layoutManager,
      let textContainer = textView.textContainer else {
    fail("could not create text layout")
}
layoutManager.ensureLayout(for: textContainer)
let used = layoutManager.usedRect(for: textContainer)
let height = max(1, ceil(used.height))
textView.frame = NSRect(x: 0, y: 0, width: printable.width, height: height)

let operation = NSPrintOperation(view: textView, printInfo: printInfo)
operation.showsPrintPanel = false
operation.showsProgressPanel = false
if !operation.run() {
    fail("could not render PDF")
}

guard FileManager.default.fileExists(atPath: output.path) else {
    fail("PDF was not created")
}
