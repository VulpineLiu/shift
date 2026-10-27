import AppKit
import Foundation
import PDFKit
import Vision
import WebKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

func hasTextLayer(_ page: PDFPage) -> Bool {
    let characters = (page.string ?? "").filter { !$0.isWhitespace }
    return characters.count >= 20
}

/// Renders a PDF page on a white background, matching FileFlipper's PDFKit path.
func renderPage(_ page: PDFPage, scale: CGFloat) -> CGImage? {
    let box = page.bounds(for: .mediaBox)
    let sideways = page.rotation % 180 != 0
    let width = Int(((sideways ? box.height : box.width) * scale).rounded())
    let height = Int(((sideways ? box.width : box.height) * scale).rounded())
    guard width > 0, height > 0,
          let context = CGContext(
            data: nil,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
          ) else {
        return nil
    }
    context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    context.interpolationQuality = .high
    context.scaleBy(x: scale, y: scale)
    page.draw(with: .mediaBox, to: context)
    return context.makeImage()
}

/// Uses Vision, the system's on-device Live Text engine, for scanned pages.
func recognizeText(_ page: PDFPage) -> String {
    guard let image = renderPage(page, scale: 3) else { return "" }
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true
    request.automaticallyDetectsLanguage = true
    let preferred = [
        "zh-Hans", "zh-Hant", "en-US", "ja-JP", "ko-KR",
        "fr-FR", "de-DE", "es-ES", "it-IT", "pt-BR", "ru-RU",
    ]
    let supported = (try? request.supportedRecognitionLanguages()) ?? []
    let languages = preferred.filter { supported.contains($0) }
    if !languages.isEmpty {
        request.recognitionLanguages = languages
    }
    do {
        try VNImageRequestHandler(cgImage: image).perform([request])
    } catch {
        return ""
    }
    let pieces = (request.results ?? []).compactMap { observation -> (CGRect, String)? in
        guard let text = observation.topCandidates(1).first?.string else { return nil }
        return (observation.boundingBox, text)
    }
    let sorted = pieces.sorted { $0.0.midY > $1.0.midY }
    var rows: [[(CGRect, String)]] = []
    for piece in sorted {
        if let last = rows.last?.first,
           abs(last.0.midY - piece.0.midY) < min(last.0.height, piece.0.height) / 2 {
            rows[rows.count - 1].append(piece)
        } else {
            rows.append([piece])
        }
    }
    return rows.map { row in
        row.sorted { $0.0.minX < $1.0.minX }.map { $0.1 }.joined(separator: " ")
    }.joined(separator: "\n")
}

/// PDF -> editable DOCX through PDFKit and Cocoa's native OOXML writer.
func writeWordDocument(from pdf: URL, to output: URL) throws {
    guard let document = PDFDocument(url: pdf) else {
        throw NSError(domain: "Shift", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "Could not open PDF: \(pdf.path)",
        ])
    }
    if document.isLocked {
        throw NSError(domain: "Shift", code: 2, userInfo: [
            NSLocalizedDescriptionKey: "This PDF is password protected.",
        ])
    }
    let combined = NSMutableAttributedString()
    for index in 0..<document.pageCount {
        guard let page = document.page(at: index) else { continue }
        let pageText: NSAttributedString
        if hasTextLayer(page), let attributed = page.attributedString {
            pageText = attributed
        } else {
            pageText = NSAttributedString(
                string: recognizeText(page),
                attributes: [.font: NSFont.systemFont(ofSize: 12)]
            )
        }
        guard pageText.length > 0 else { continue }
        if combined.length > 0 {
            combined.append(NSAttributedString(string: "\n\u{0C}"))
        }
        combined.append(pageText)
    }
    guard combined.length > 0 else {
        throw NSError(domain: "Shift", code: 3, userInfo: [
            NSLocalizedDescriptionKey: "No text could be read from this PDF.",
        ])
    }
    let data = try combined.data(
        from: NSRange(location: 0, length: combined.length),
        documentAttributes: [.documentType: NSAttributedString.DocumentType.officeOpenXML]
    )
    try data.write(to: output, options: .atomic)
}

guard (3...5).contains(CommandLine.arguments.count) else {
    fail("usage: doc-render <input> <output> [html-base-dir] [--webkit]")
}

let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)

if output.pathExtension.lowercased() == "docx" {
    guard input.pathExtension.lowercased() == "pdf" else {
        fail("PDF-to-Word input must be a PDF")
    }
    do {
        try writeWordDocument(from: input, to: output)
        exit(0)
    } catch {
        fail(error.localizedDescription)
    }
}

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
    let baseDirectory = CommandLine.arguments.count == 4
        ? URL(fileURLWithPath: CommandLine.arguments[3], isDirectory: true)
        : CommandLine.arguments.count == 5
            ? URL(fileURLWithPath: CommandLine.arguments[3], isDirectory: true)
        : input.deletingLastPathComponent()
    options[.baseURL] = baseDirectory
}

if documentType == .html,
   CommandLine.arguments.count == 5,
   CommandLine.arguments[4] == "--webkit" {
    let baseDirectory = URL(fileURLWithPath: CommandLine.arguments[3], isDirectory: true)
    let renderer = HTMLRenderer(input: input, output: output, baseDirectory: baseDirectory)
    renderer.run()
    app.run()
    exit(0)
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

/// Fits every picture into the width of the printed page.
///
/// The layout sizes an attachment from the density header inside the file —
/// 72 dpi is assumed when the file carries none — and the cell AppKit builds
/// for an `<img>` honours neither `bounds` nor a CSS width, so a 1600 px
/// screenshot saved at 96 dpi asks for 1200 points and prints as a cropped
/// fragment of itself. A plain `NSTextAttachment` does honour `bounds`, and the
/// print operation then scales it onto the page along with the text. Aspect
/// ratio is kept and nothing is resampled: only the size the layout asks for
/// changes.
func fitAttachments(
    _ source: NSAttributedString,
    to containerWidth: CGFloat
) -> NSMutableAttributedString {
    let mutable = NSMutableAttributedString(attributedString: source)
    let wanted = max(1, containerWidth)
    mutable.enumerateAttribute(
        .attachment,
        in: NSRange(location: 0, length: mutable.length),
        options: []
    ) { value, range, _ in
        guard let old = value as? NSTextAttachment,
              let contents = old.fileWrapper?.regularFileContents,
              let image = NSImage(data: contents),
              image.size.width > wanted else { return }
        let scale = wanted / image.size.width
        let replacement = NSTextAttachment()
        replacement.image = image
        replacement.bounds = NSRect(
            x: 0,
            y: 0,
            width: (image.size.width * scale).rounded(),
            height: (image.size.height * scale).rounded()
        )
        mutable.addAttribute(.attachment, value: replacement, range: range)
    }
    return mutable
}

/// Writes the tall PDF WebKit produces from the page as A4-sized slices.
///
/// `createPDF` keeps the HTML layout as one continuous page. Printing the web
/// view directly can loop indefinitely on documents with positioned elements,
/// so draw the single page into clipped A4 pages instead.
func writePaginatedPDF(_ data: Data, to output: URL) {
    let pageSize = CGSize(width: 595.2, height: 841.8)
    guard let provider = CGDataProvider(data: data as CFData),
          let document = CGPDFDocument(provider),
          let source = document.page(at: 1) else {
        fail("could not read the rendered web page")
    }
    let sourceBounds = source.getBoxRect(.mediaBox)
    let height = max(1, sourceBounds.height)
    let pageCount = max(1, Int(ceil(height / pageSize.height)))
    var mediaBox = CGRect(origin: .zero, size: pageSize)
    guard let context = CGContext(output as CFURL, mediaBox: &mediaBox, nil) else {
        fail("could not create the PDF")
    }
    for index in 0..<pageCount {
        context.beginPDFPage(nil)
        context.saveGState()
        context.clip(to: mediaBox)
        let offset = max(0, height - CGFloat(index + 1) * pageSize.height)
        let remaining = max(0, height - CGFloat(index) * pageSize.height)
        // Full slices move up through the tall page. The final short slice is
        // top-aligned so the trailing whitespace sits below the content.
        let translationY = remaining < pageSize.height
            ? pageSize.height - remaining
            : -offset
        context.translateBy(x: 0, y: translationY)
        context.drawPDFPage(source)
        context.restoreGState()
        context.endPDFPage()
    }
    context.closePDF()
}

/// Renders HTML with WebKit instead of flattening it into an attributed
/// string. Quick Look supplies Word page furniture and positioned tables as
/// HTML; AppKit's text conversion reflows that content, which changes the page
/// count and can move a footer onto a page of its own.
final class HTMLRenderer: NSObject, WKNavigationDelegate {
    private let input: URL
    private let baseDirectory: URL
    private let output: URL
    private let webView: WKWebView
    private var finished = false

    init(input: URL, output: URL, baseDirectory: URL) {
        self.input = input
        self.output = output
        self.baseDirectory = baseDirectory
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        self.webView = WKWebView(
            frame: NSRect(x: 0, y: 0, width: 595.2, height: 841.8),
            configuration: configuration
        )
        super.init()
        webView.navigationDelegate = self
    }

    func run() {
        webView.loadFileURL(input, allowingReadAccessTo: baseDirectory)
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            guard !self.finished else { return }
            self.finished = true
            fail("could not render HTML before the 30 second deadline")
        }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard !finished else { return }
        webView.evaluateJavaScript(
            """
            (() => {
              const pageWidth = document.documentElement.clientWidth;
              for (const image of document.images) {
                if (image.getBoundingClientRect().width > pageWidth) {
                  image.style.width = `${pageWidth}px`;
                  image.style.height = "auto";
                  image.style.maxWidth = "100%";
                }
              }
              let contentBottom = 0;
              const range = document.createRange();
              const walker = document.createTreeWalker(
                document.body,
                NodeFilter.SHOW_TEXT
              );
              for (let node = walker.nextNode(); node; node = walker.nextNode()) {
                if (!node.textContent.trim()) continue;
                range.selectNodeContents(node);
                const rect = range.getBoundingClientRect();
                if (rect.width > 0 && rect.height > 0) {
                  contentBottom = Math.max(
                    contentBottom,
                    rect.bottom + window.scrollY
                  );
                }
              }
              for (const element of document.querySelectorAll(
                "img, svg, canvas, table, hr"
              )) {
                const rect = element.getBoundingClientRect();
                if (rect.width > 0 && rect.height > 0) {
                  contentBottom = Math.max(
                    contentBottom,
                    rect.bottom + window.scrollY
                  );
                }
              }
              return {
                width: document.documentElement.scrollWidth,
                height: Math.max(1, Math.ceil(contentBottom))
              };
            })()
            """
        ) { value, error in
            guard !self.finished else { return }
            self.finished = true
            guard error == nil,
                  let size = value as? [String: Any],
                  let width = (size["width"] as? NSNumber)?.doubleValue,
                  let height = (size["height"] as? NSNumber)?.doubleValue,
                  width > 0,
                  height > 0 else {
                fail("could not measure the rendered HTML")
            }
            let configuration = WKPDFConfiguration()
            configuration.rect = CGRect(x: 0, y: 0, width: width, height: height)
            webView.createPDF(configuration: configuration) { result in
                switch result {
                case .success(let data):
                    writePaginatedPDF(data, to: self.output)
                    NSApp.terminate(nil)
                case .failure(let error):
                    fail("could not print HTML: \(error.localizedDescription)")
                }
            }
        }
    }

    func webView(
        _ webView: WKWebView,
        didFail navigation: WKNavigation!,
        withError error: Error
    ) {
        guard !finished else { return }
        finished = true
        fail("could not load HTML: \(error.localizedDescription)")
    }

    func webView(
        _ webView: WKWebView,
        didFailProvisionalNavigation navigation: WKNavigation!,
        withError error: Error
    ) {
        guard !finished else { return }
        finished = true
        fail("could not load HTML: \(error.localizedDescription)")
    }
}

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
let fitted = fitAttachments(attributed, to: printable.width)
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
textView.textStorage?.setAttributedString(fitted)

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
