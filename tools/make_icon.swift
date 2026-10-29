// Builds MicDeck's artwork from app/Resources/source/logo-reference.jpg (the metallic logo on a dark background):
//   app/Resources/AppIcon.png            1024 px app icon (dark tile + logo); AppIcon.icns is made from it, see CLAUDE.md
//   app/Sources/MicDeck/Resources/MenuBarIcon.png   one-colour silhouette for the menu bar (macOS recolors it)
//   app/Resources/dmg-background.png     background of the .dmg window (1320x800 = 660x400 points at 2x)
// Run from the repo root: swift tools/make_icon.swift
import AppKit

let source = NSBitmapImageRep(data: try! Data(contentsOf: URL(fileURLWithPath: "app/Resources/source/logo-reference.jpg")))!

func luminance(_ c: NSColor) -> Double { 0.299 * c.redComponent + 0.587 * c.greenComponent + 0.114 * c.blueComponent }
/// 0 below `a`, 1 above `b`, smooth in between.
func smoothstep(_ a: Double, _ b: Double, _ x: Double) -> Double { let t = min(max((x - a) / (b - a), 0), 1); return t * t * (3 - 2 * t) }

// 1. Cut the logo out of the background. The background is dark (brightness 30-60 of 255) and the logo is
//    lighter (80 and up), so brightness decides how opaque each pixel is. Only the area around the logo is searched.
let area = (x: 480, y: 140, w: 450, h: 450)
let cut = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: area.w, pixelsHigh: area.h, bitsPerSample: 8, samplesPerPixel: 4,
                           hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
var box = (minX: area.w, minY: area.h, maxX: 0, maxY: 0) // the tight box around the logo
for y in 0..<area.h {
    for x in 0..<area.w {
        let c = source.colorAt(x: area.x + x, y: area.y + y)!.usingColorSpace(.deviceRGB)!
        let alpha = smoothstep(68.0 / 255, 92.0 / 255, luminance(c))
        cut.setColor(NSColor(deviceRed: c.redComponent, green: c.greenComponent, blue: c.blueComponent, alpha: alpha), atX: x, y: y)
        if alpha > 0.5 { box = (min(box.minX, x), min(box.minY, y), max(box.maxX, x), max(box.maxY, y)) }
    }
}
let logoRect = NSRect(x: box.minX, y: box.minY, width: box.maxX - box.minX + 1, height: box.maxY - box.minY + 1)
let logo = NSImage(size: NSSize(width: area.w, height: area.h)); logo.addRepresentation(cut)
/// Draw the logo (tight crop, original colours) centered in `target`, as large as fits.
func drawLogo(in target: NSRect, silhouette: Bool = false) {
    // The cut-out's rows run top to bottom, drawing coordinates bottom to top, so flip the source rectangle.
    let from = NSRect(x: logoRect.minX, y: CGFloat(area.h) - logoRect.maxY, width: logoRect.width, height: logoRect.height)
    let scale = min(target.width / from.width, target.height / from.height)
    let size = NSSize(width: from.width * scale, height: from.height * scale)
    let to = NSRect(x: target.midX - size.width / 2, y: target.midY - size.height / 2, width: size.width, height: size.height)
    logo.draw(in: to, from: from, operation: .sourceOver, fraction: 1)
    if silhouette { // paint everything we just drew black, keeping its transparency
        NSColor.black.set(); to.fill(using: .sourceAtop)
    }
}

/// Make a picture `w` x `h` pixels, let `draw` paint into it, and save it as PNG.
func png(_ path: String, _ w: Int, _ h: Int, _ draw: () -> Void) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4,
                               hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    draw()
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
}

let darkTop = NSColor(deviceRed: 0.20, green: 0.22, blue: 0.27, alpha: 1), darkBottom = NSColor(deviceRed: 0.08, green: 0.09, blue: 0.11, alpha: 1)

// 2. App icon: macOS leaves a margin around the tile, so the tile is 824 of the 1024 pixels.
png("app/Resources/AppIcon.png", 1024, 1024) {
    let tile = NSBezierPath(roundedRect: NSRect(x: 100, y: 100, width: 824, height: 824), xRadius: 185, yRadius: 185)
    NSGraphicsContext.saveGraphicsState()
    let shadow = NSShadow(); shadow.shadowColor = NSColor.black.withAlphaComponent(0.5); shadow.shadowBlurRadius = 24; shadow.shadowOffset = NSSize(width: 0, height: -10)
    shadow.set(); darkBottom.setFill(); tile.fill()
    NSGraphicsContext.restoreGraphicsState()
    NSGradient(starting: darkTop, ending: darkBottom)!.draw(in: tile, angle: -90)
    NSColor.white.withAlphaComponent(0.12).setStroke(); tile.lineWidth = 3; tile.stroke()
    drawLogo(in: NSRect(x: 512 - 270, y: 512 - 270, width: 540, height: 540))
}

// 3. Menu-bar silhouette, 2x resolution of the 18-point icon.
png("app/Sources/MicDeck/Resources/MenuBarIcon.png", 36, 36) {
    drawLogo(in: NSRect(x: 1, y: 1, width: 34, height: 34), silhouette: true)
}

// 4. The .dmg window background: logo, an arrow from the app to the Applications folder, and the BlackHole note.
png("app/Resources/dmg-background.png", 1320, 800) {
    NSGradient(starting: darkTop, ending: darkBottom)!.draw(in: NSRect(x: 0, y: 0, width: 1320, height: 800), angle: -90)
    drawLogo(in: NSRect(x: 560, y: 540, width: 200, height: 200))
    let centered = NSMutableParagraphStyle(); centered.alignment = .center
    func text(_ s: String, size: CGFloat, alpha: CGFloat, y: CGFloat) {
        s.draw(in: NSRect(x: 60, y: y, width: 1200, height: size * 1.6), withAttributes: [
            .font: NSFont.systemFont(ofSize: size, weight: .medium), .foregroundColor: NSColor.white.withAlphaComponent(alpha), .paragraphStyle: centered])
    }
    text("Drag MicDeck onto Applications", size: 40, alpha: 0.9, y: 460)
    // The two icons (96 pt) sit at (330, 360) and (990, 360) in this picture (bottom-up pixels); the arrow goes between them.
    let arrow = NSBezierPath(); arrow.lineWidth = 8; arrow.lineCapStyle = .round; arrow.lineJoinStyle = .round
    arrow.move(to: NSPoint(x: 520, y: 360)); arrow.line(to: NSPoint(x: 800, y: 360))
    arrow.move(to: NSPoint(x: 760, y: 400)); arrow.line(to: NSPoint(x: 800, y: 360)); arrow.line(to: NSPoint(x: 760, y: 320))
    NSColor.white.withAlphaComponent(0.5).setStroke(); arrow.stroke()
    // Finder always writes icon names in dark text, so give each name a light chip to be readable on the dark picture.
    for x in [330.0, 990.0] {
        NSColor.white.withAlphaComponent(0.88).setFill()
        NSBezierPath(roundedRect: NSRect(x: x - 130, y: 204, width: 260, height: 56), xRadius: 14, yRadius: 14).fill()
    }
    text("First time opening it? System Settings > Privacy & Security > Open Anyway.", size: 26, alpha: 0.7, y: 140)
    text("Meeting mode (optional) needs BlackHole 2ch, a free driver by another developer.", size: 26, alpha: 0.7, y: 96)
    text("Steps and links: github.com/ikarthikng/micdeck", size: 26, alpha: 0.7, y: 52)
}

// 5. GitHub's social preview (1280x640), shown when the repo link is shared. Upload it by hand in the repo's
//    Settings > General > Social preview; GitHub has no way to set it from a file in the repo.
png("app/Resources/social-preview.png", 1280, 640) {
    NSGradient(starting: darkTop, ending: darkBottom)!.draw(in: NSRect(x: 0, y: 0, width: 1280, height: 640), angle: -90)
    drawLogo(in: NSRect(x: 110, y: 110, width: 420, height: 420))
    "MicDeck".draw(at: NSPoint(x: 590, y: 330), withAttributes: [.font: NSFont.systemFont(ofSize: 110, weight: .bold), .foregroundColor: NSColor.white])
    "Control software for USB mics on macOS.\nLights, mute indicator and meeting-mode noise cleanup.".draw(in: NSRect(x: 594, y: 190, width: 620, height: 110), withAttributes: [
        .font: NSFont.systemFont(ofSize: 30, weight: .regular), .foregroundColor: NSColor.white.withAlphaComponent(0.7)])
}
