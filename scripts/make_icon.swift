// Renders assets/icon.png (1024x1024) and assets/icon-256.png: the
// splice9000 mark, a light strip and a splice-orange strip joined at a
// diagonal cut, on a graphite body.
// Run: swift scripts/make_icon.swift
import AppKit

func render(_ size: Int) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let ctx = NSGraphicsContext.current!.cgContext
    let k = CGFloat(size) / 1024
    ctx.scaleBy(x: k, y: k)
    func color(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
        NSColor(srgbRed: CGFloat(hex >> 16 & 0xff) / 255, green: CGFloat(hex >> 8 & 0xff) / 255,
                blue: CGFloat(hex & 0xff) / 255, alpha: a).cgColor
    }

    // Body on Apple's icon grid: 824 square, 100 inset.
    let body = CGPath(roundedRect: CGRect(x: 100, y: 100, width: 824, height: 824), cornerWidth: 185, cornerHeight: 185, transform: nil)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: color(0, 0.35))
    ctx.addPath(body)
    ctx.setFillColor(color(0x17181b))
    ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.addPath(body)
    ctx.clip()
    let grad = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: [color(0x22242a), color(0x121316)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 0, y: 924), end: CGPoint(x: 0, y: 100), options: [])
    // A hairline track under the strips.
    ctx.setFillColor(color(0x2e3036))
    ctx.fill(CGRect(x: 100, y: 511, width: 824, height: 3))
    ctx.restoreGState()

    // The two strips meet at a slanted cut, a small gap between them.
    let (y0, y1): (CGFloat, CGFloat) = (432, 592)
    let (left, right): (CGFloat, CGFloat) = (226, 798)
    let slant: CGFloat = 46, gap: CGFloat = 22, mid: CGFloat = 512
    func strip(_ points: [CGPoint], _ fill: CGColor) {
        let p = CGMutablePath()
        p.addLines(between: points)
        p.closeSubpath()
        ctx.saveGState()
        // Rounded outer ends: clip to a capsule over the whole bar.
        ctx.addPath(CGPath(roundedRect: CGRect(x: left, y: y0, width: right - left, height: y1 - y0), cornerWidth: 34, cornerHeight: 34, transform: nil))
        ctx.clip()
        ctx.addPath(p)
        ctx.setFillColor(fill)
        ctx.fillPath()
        ctx.restoreGState()
    }
    strip([CGPoint(x: left, y: y0), CGPoint(x: mid - gap / 2 - slant, y: y0), CGPoint(x: mid - gap / 2 + slant, y: y1), CGPoint(x: left, y: y1)], color(0xe8e9eb))
    strip([CGPoint(x: mid + gap / 2 - slant, y: y0), CGPoint(x: right, y: y0), CGPoint(x: right, y: y1), CGPoint(x: mid + gap / 2 + slant, y: y1)], color(0xff5c39))
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let dir = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "assets")
try! render(1024).representation(using: .png, properties: [:])!.write(to: dir.appendingPathComponent("icon.png"))
try! render(256).representation(using: .png, properties: [:])!.write(to: dir.appendingPathComponent("icon-256.png"))
