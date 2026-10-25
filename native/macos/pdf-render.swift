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
    // Render into an explicit pixel buffer. NSImage.lockFocus() follows the
    // Mac's Retina backing scale, which doubled both dimensions and made
    // image-heavy PDF-to-Word conversions needlessly slow.
    let scale = min(2.0, 1800.0 / max(bounds.width, bounds.height, 1.0))
    let width = max(1, Int((bounds.width * scale).rounded()))
    let height = max(1, Int((bounds.height * scale).rounded()))
    guard let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
          let context = CGContext(
            data: nil,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
          ) else {
        fail("could not create page canvas")
    }
    context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    context.scaleBy(x: CGFloat(width) / bounds.width, y: CGFloat(height) / bounds.height)
    page.draw(with: .mediaBox, to: context)
    guard let image = context.makeImage(),
          let data = NSBitmapImageRep(cgImage: image).representation(
            using: fileType,
            properties: [:]
          ) else {
        fail("could not render page \(index + 1)")
    }
    let output = outputDirectory.appendingPathComponent(
        String(format: "Page %03d.%@", index + 1, extensionName == "jpeg" ? "jpg" : extensionName)
    )
    try data.write(to: output)
}
