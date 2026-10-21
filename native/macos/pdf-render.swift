import AppKit
import Foundation
import PDFKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

guard CommandLine.arguments.count == 4 else {
    fail("usage: pdf-render <input.pdf> <output-dir> <png|jpg|tiff>")
}

let input = URL(fileURLWithPath: CommandLine.arguments[1])
let outputDirectory = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
let extensionName = CommandLine.arguments[3].lowercased()
let fileType: NSBitmapImageRep.FileType
switch extensionName {
case "png": fileType = .png
case "jpg", "jpeg": fileType = .jpeg
case "tif", "tiff": fileType = .tiff
default: fail("unsupported image format: \(extensionName)")
}

try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)
guard let document = PDFDocument(url: input) else {
    fail("could not open PDF: \(input.path)")
}

for index in 0..<document.pageCount {
    guard let page = document.page(at: index) else { continue }
    let bounds = page.bounds(for: .mediaBox)
    let scale = min(2.0, 3000.0 / max(bounds.width, bounds.height, 1.0))
    let size = NSSize(
        width: max(1, bounds.width * scale),
        height: max(1, bounds.height * scale)
    )
    let pageImage = page.thumbnail(of: size, for: .mediaBox)
    let canvas = NSImage(size: size)
    canvas.lockFocus()
    NSColor.white.setFill()
    NSRect(origin: .zero, size: size).fill()
    pageImage.draw(in: NSRect(origin: .zero, size: size))
    canvas.unlockFocus()

    guard let tiff = canvas.tiffRepresentation,
          let bitmap = NSBitmapImageRep(data: tiff),
          let data = bitmap.representation(using: fileType, properties: [:]) else {
        fail("could not render page \(index + 1)")
    }
    let output = outputDirectory.appendingPathComponent(
        String(format: "Page %03d.%@", index + 1, extensionName == "jpeg" ? "jpg" : extensionName)
    )
    try data.write(to: output)
}
