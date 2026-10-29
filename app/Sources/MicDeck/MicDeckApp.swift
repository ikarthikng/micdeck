import SwiftUI
import CMicDeck

/// The menu-bar app. `MenuBarExtra` puts an icon in the menu bar; the window style shows our SwiftUI view in a popover.
@main
struct MicDeckApp: App {
    @StateObject private var model = Model()

    init() {
        // Menu-bar only: no Dock icon (the finished .app will say the same in its Info.plist).
        NSApplication.shared.setActivationPolicy(.accessory)
    }

    var body: some Scene {
        MenuBarExtra {
            PanelView(model: model)
        } label: {
            Image(nsImage: menuBarIcon(muted: model.muted))
        }
        .menuBarExtraStyle(.window)
    }
}

/// Everything the panel shows, and what the buttons do. Each button calls one function of the Rust library.
final class Model: ObservableObject {
    @Published var muted = false
    @Published var effect = "solid" { didSet { applyLight() } }
    @Published var color = Color.orange { didSet { applyLight() } }
    @Published var brightness = 100.0 { didSet { applyLight() } }
    @Published var lightOn = false
    @Published var marker = CGPoint(x: 0.5, y: 0.5) // where the color wheel's dot sits, 0...1 across the wheel
    @Published var error: String?

    // Meeting mode. The choices are remembered between runs in the app's own preferences (UserDefaults).
    @Published var meetingOn = false
    @Published var strength = Double(UserDefaults.standard.object(forKey: "strength") as? Int ?? 60) { didSet { UserDefaults.standard.set(Int(strength), forKey: "strength") } }
    @Published var input = UserDefaults.standard.string(forKey: "input") ?? "HyperX DuoCast" { didSet { UserDefaults.standard.set(input, forKey: "input"); applyMeeting() } }
    @Published var output = UserDefaults.standard.string(forKey: "output") ?? "BlackHole 2ch" { didSet { UserDefaults.standard.set(output, forKey: "output"); applyMeeting() } }
    @Published var hasBlackHole = false // Meeting mode needs a virtual mic; we rely on BlackHole
    @Published var duocastConnected = false // the mic the lights and mute button belong to
    @Published var inputs: [String] = []
    @Published var outputs: [String] = []

    static let swatches: [Color] = [.red, .orange, .yellow, .green, .cyan, .blue, .purple, .pink, .white]
    static let effects = ["solid", "breathing", "cycle", "confetti", "sun", "twilight"]

    init() {
        // Ask the Rust library to call us on every mute-button press. It calls from another thread, so hop to
        // the main thread before touching anything the screen shows. `Unmanaged` turns `self` into the plain
        // pointer C can hold; the Model lives as long as the app, so the pointer stays valid.
        let me = Unmanaged.passUnretained(self).toOpaque()
        md_mute_watch({ ctx, muted in
            let model = Unmanaged<Model>.fromOpaque(ctx!).takeUnretainedValue()
            DispatchQueue.main.async { model.muted = muted }
        }, me)
        // Rust threads report failures asynchronously, so look for new messages twice a second.
        Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in self?.poll() }
        // The device lists change when something is plugged in, so refresh them every 2 seconds.
        refreshDevices()
        Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in self?.refreshDevices() }
    }

    /// Read a list of device names (one per line) from the Rust library.
    private func devices(inputs: Bool) -> [String] {
        guard let p = md_devices(inputs) else { return [] }
        defer { md_free_string(p) }
        return String(cString: p).split(separator: "\n").map(String.init)
    }

    /// Update the mic and output lists. The chosen names always stay in the lists, even if that device is unplugged.
    private func refreshDevices() {
        let ins = devices(inputs: true), outs = devices(inputs: false)
        duocastConnected = ins.contains { $0.localizedCaseInsensitiveContains("duocast") }
        hasBlackHole = outs.contains { $0.localizedCaseInsensitiveContains("blackhole") }
        inputs = ins.contains(input) ? ins : ins + [input]
        outputs = outs.contains(output) ? outs : outs + [output]
    }

    /// Switch meeting mode on/off.
    func setMeeting(_ on: Bool) {
        meetingOn = on
        if on { applyMeeting() } else { md_meeting_stop() }
    }

    /// (Re)start meeting mode with the current choices. Does nothing while it is switched off.
    func applyMeeting() {
        guard meetingOn else { return }
        md_meeting_start(input, output, UInt8(strength))
    }

    /// Show the newest error from the Rust side, and notice if the light job has ended by itself.
    private func poll() {
        if let p = md_take_error() {
            error = String(cString: p)
            md_free_string(p)
        }
        if lightOn && !md_light_running() { lightOn = false }
        if meetingOn && !md_meeting_running() { meetingOn = false }
    }

    /// Switch the lights on/off.
    func setLight(_ on: Bool) {
        lightOn = on
        if on { applyLight() } else { md_light_stop() }
    }

    /// Send the current effect, color and brightness to the mic (only while the lights are switched on).
    private func applyLight() {
        guard lightOn else { return }
        let c = NSColor(color).usingColorSpace(.sRGB) ?? .orange
        _ = md_light_start(effect, UInt8(c.redComponent * 255), UInt8(c.greenComponent * 255), UInt8(c.blueComponent * 255), UInt8(brightness))
    }
}

struct PanelView: View {
    @ObservedObject var model: Model

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label(model.muted ? "Muted" : "Live", systemImage: model.muted ? "mic.slash.fill" : "mic.fill")
                    .font(.headline)
                    .foregroundStyle(model.muted ? .red : .green)
                Spacer()
                Button("Quit") { md_light_stop(); md_meeting_stop(); NSApp.terminate(nil) }
                    .tint(.red)
                    .buttonStyle(.borderedProminent)
            }
            Text(model.duocastConnected ? "HyperX DuoCast" : "HyperX DuoCast: not connected").font(.caption).foregroundStyle(.secondary) // the only mic with lights and a mute button so far
            Divider()
            Toggle("Lights", isOn: Binding(get: { model.lightOn }, set: model.setLight))
            Picker("Effect", selection: $model.effect) {
                ForEach(Model.effects, id: \.self) { Text($0.capitalized).tag($0) }
            }
            // Swatches instead of the system ColorPicker: its separate color window makes the menu-bar panel close.
            HStack {
                Text("Color")
                Spacer()
                ForEach(Model.swatches, id: \.self) { c in
                    Circle().fill(c).frame(width: 18, height: 18)
                        .overlay(Circle().stroke(Color.primary, lineWidth: model.color == c ? 2 : 0))
                        .onTapGesture { model.color = c }
                }
            }
            // Always visible: the menu-bar panel does not resize when a section expands, so it got cut off.
            ColorWheel(model: model)
                .frame(width: 130, height: 130)
                .frame(maxWidth: .infinity)
            HStack {
                Text("Brightness")
                Slider(value: $model.brightness, in: 0...100)
            }
            Divider()
            Toggle("Meeting mode", isOn: Binding(get: { model.meetingOn }, set: model.setMeeting))
                .disabled(!model.hasBlackHole)
            if !model.hasBlackHole {
                Text("Meeting mode needs BlackHole 2ch, a free audio driver from another developer. MicDeck does not install it; read about it first, then install it yourself if you want Meeting mode.")
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                HStack {
                    Link("BlackHole website", destination: URL(string: "https://existential.audio/blackhole/")!)
                    Link("Source code", destination: URL(string: "https://github.com/ExistentialAudio/BlackHole")!)
                }.font(.caption)
            }
            Picker("Mic", selection: $model.input) {
                ForEach(model.inputs, id: \.self) { Text($0).tag($0) }
            }
            Picker("Send to", selection: $model.output) {
                ForEach(model.outputs, id: \.self) { Text($0).tag($0) }
            }
            HStack {
                Text("Strength")
                // Restart only when the slider is let go, so the audio is not interrupted while dragging.
                Slider(value: $model.strength, in: 0...100, onEditingChanged: { editing in if !editing { model.applyMeeting() } })
                Text("\(Int(model.strength))").monospacedDigit().frame(width: 28, alignment: .trailing)
            }
            if let e = model.error {
                Text(e).font(.caption).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(14)
        .frame(width: 260)
    }
}

/// A color wheel drawn inside the panel (the system picker opens a separate window, which closes the panel).
/// The angle around the wheel is the hue (the color), the distance from the middle is the saturation (0 = white).
struct ColorWheel: View {
    @ObservedObject var model: Model

    var body: some View {
        GeometryReader { geo in
            let size = min(geo.size.width, geo.size.height)
            ZStack {
                Circle().fill(AngularGradient(gradient: Gradient(colors: (0...12).map { Color(hue: Double($0) / 12, saturation: 1, brightness: 1) }), center: .center))
                Circle().fill(RadialGradient(colors: [.white, .white.opacity(0)], center: .center, startRadius: 0, endRadius: size / 2))
                Circle().stroke(Color.primary, lineWidth: 2).frame(width: 12, height: 12)
                    .position(x: model.marker.x * size, y: model.marker.y * size)
            }
            .frame(width: size, height: size)
            .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                let r = size / 2
                var dx = drag.location.x - r, dy = drag.location.y - r
                let dist = sqrt(dx * dx + dy * dy)
                if dist > r { dx *= r / dist; dy *= r / dist } // keep the dot inside the wheel
                // The gradient starts at 3 o'clock and runs clockwise, which is what atan2 gives with y pointing down.
                let hue = (atan2(dy, dx) / (2 * .pi)).truncatingRemainder(dividingBy: 1)
                model.color = Color(hue: hue < 0 ? hue + 1 : hue, saturation: min(dist / r, 1), brightness: 1)
                model.marker = CGPoint(x: (r + dx) / size, y: (r + dy) / size)
            })
        }
    }
}

/// The menu-bar icon, drawn in code: a microphone with sound bars on both sides, and a slash through it when
/// muted. It is a "template" image: macOS recolors it for light/dark menu bars, so we only draw the shape.
func menuBarIcon(muted: Bool) -> NSImage {
    let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { _ in
        NSColor.black.set()
        // The microphone head.
        NSBezierPath(roundedRect: NSRect(x: 6.5, y: 7, width: 5, height: 9), xRadius: 2.5, yRadius: 2.5).fill()
        // The holder under it, the stem, and the sound bars to the left and right.
        let lines = NSBezierPath()
        lines.lineWidth = 1.5
        lines.lineCapStyle = .round
        lines.appendArc(withCenter: NSPoint(x: 9, y: 9), radius: 5, startAngle: 180, endAngle: 360)
        for (from, to) in [((9.0, 4.0), (9.0, 2.0)), ((1.5, 8.0), (1.5, 11.0)), ((16.5, 8.0), (16.5, 11.0))] {
            lines.move(to: NSPoint(x: from.0, y: from.1))
            lines.line(to: NSPoint(x: to.0, y: to.1))
        }
        lines.stroke()
        if muted {
            // A slash with a small gap around it: first erase a wide stripe, then draw the thinner line over it.
            let slash = NSBezierPath()
            slash.move(to: NSPoint(x: 2, y: 16))
            slash.line(to: NSPoint(x: 16, y: 2))
            slash.lineCapStyle = .round
            NSGraphicsContext.current?.compositingOperation = .clear
            slash.lineWidth = 4
            slash.stroke()
            NSGraphicsContext.current?.compositingOperation = .sourceOver
            slash.lineWidth = 1.5
            slash.stroke()
        }
        return true
    }
    image.isTemplate = true
    return image
}
