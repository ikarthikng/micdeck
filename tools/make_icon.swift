// Draws the MicDeck app icon (a microphone on a mixing deck, neon gradient on black) and writes
// app/Resources/AppIcon.png (1024 px) and AppIcon.icns. Run from the repo root: swift tools/make_icon.swift
import SwiftUI
import AppKit

let gradient = LinearGradient(colors: [Color(red: 0.1, green: 0.8, blue: 1.0), Color(red: 0.45, green: 0.45, blue: 1.0), Color(red: 0.85, green: 0.3, blue: 0.95)],
                              startPoint: .topLeading, endPoint: .bottomTrailing)

/// A shape with a white outline and the gradient inside, like the reference picture.
struct Outlined<S: Shape>: View {
    let shape: S
    var body: some View {
        shape.fill(gradient).overlay(shape.stroke(Color.white, lineWidth: 14))
    }
}

/// The deck: a trapezoid wider at the bottom, to look like it lies on a table.
struct Deck: Shape {
    func path(in r: CGRect) -> Path {
        var p = Path()
        p.move(to: CGPoint(x: 350, y: 630)); p.addLine(to: CGPoint(x: 674, y: 630))
        p.addLine(to: CGPoint(x: 735, y: 775)); p.addLine(to: CGPoint(x: 289, y: 775)); p.closeSubpath()
        return p
    }
}

struct Icon: View {
    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.clear
            // Background tile with a glowing gradient edge. macOS app icons leave a margin around the tile.
            RoundedRectangle(cornerRadius: 185).fill(Color.black).frame(width: 824, height: 824).offset(x: 100, y: 100)
            RoundedRectangle(cornerRadius: 185).stroke(gradient, lineWidth: 14).frame(width: 824, height: 824).offset(x: 100, y: 100)
                .shadow(color: Color(red: 0.4, green: 0.5, blue: 1).opacity(0.7), radius: 18)

            // Deck with a grid and four faders.
            Outlined(shape: Deck())
            Path { p in
                for x in [1.0, 2.0, 3.0] { // vertical lines inside the deck, slanting with the trapezoid
                    p.move(to: CGPoint(x: 350 + 324 * x / 4, y: 630)); p.addLine(to: CGPoint(x: 289 + 446 * x / 4, y: 775))
                }
            }.stroke(Color.black, lineWidth: 9)
            Path { p in
                for (x, y) in [(375.0, 710.0), (660.0, 710.0)] {
                    p.move(to: CGPoint(x: x, y: y - 24)); p.addLine(to: CGPoint(x: x - 7, y: y + 26))
                    p.move(to: CGPoint(x: x - 20, y: y)); p.addLine(to: CGPoint(x: x + 10, y: y))
                }
            }.stroke(Color.black, style: StrokeStyle(lineWidth: 12, lineCap: .round))

            // Stem and its round foot.
            Outlined(shape: Ellipse()).frame(width: 130, height: 46).offset(x: 447, y: 612)
            Outlined(shape: RoundedRectangle(cornerRadius: 12)).frame(width: 42, height: 90).offset(x: 491, y: 540)

            // Holder (the U around the microphone).
            Path { p in
                p.move(to: CGPoint(x: 372, y: 400))
                p.addArc(center: CGPoint(x: 512, y: 400), radius: 140, startAngle: .degrees(180), endAngle: .degrees(0), clockwise: true)
            }.stroke(Color.white, style: StrokeStyle(lineWidth: 38, lineCap: .round))
            Path { p in
                p.move(to: CGPoint(x: 372, y: 400))
                p.addArc(center: CGPoint(x: 512, y: 400), radius: 140, startAngle: .degrees(180), endAngle: .degrees(0), clockwise: true)
            }.stroke(gradient, style: StrokeStyle(lineWidth: 16, lineCap: .round))

            // Microphone head: a rounded top with slots.
            Outlined(shape: UnevenRoundedRectangle(topLeadingRadius: 110, bottomLeadingRadius: 70, bottomTrailingRadius: 70, topTrailingRadius: 110))
                .frame(width: 230, height: 300).offset(x: 397, y: 215)
            Path { p in
                for dx in [-66.0, -33.0, 0.0, 33.0, 66.0] {
                    p.move(to: CGPoint(x: 512 + dx, y: 345)); p.addLine(to: CGPoint(x: 512 + dx, y: 470))
                }
            }.stroke(Color.black, style: StrokeStyle(lineWidth: 16, lineCap: .round))
        }
        .frame(width: 1024, height: 1024)
    }
}

@MainActor func render() {
    let renderer = ImageRenderer(content: Icon())
    renderer.scale = 1
    guard let image = renderer.nsImage, let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
          let png = rep.representation(using: .png, properties: [:]) else { fatalError("could not render the icon") }
    try! png.write(to: URL(fileURLWithPath: "app/Resources/AppIcon.png"))
}
MainActor.assumeIsolated { render() }
