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
        MenuBarExtra("MicDeck", systemImage: model.muted ? "mic.slash.fill" : "mic.fill") {
            PanelView(model: model)
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
    }

    /// Show the newest error from the Rust side, and notice if the light job has ended by itself.
    private func poll() {
        if let p = md_take_error() {
            error = String(cString: p)
            md_free_string(p)
        }
        if lightOn && !md_light_running() { lightOn = false }
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
            Label(model.muted ? "Muted" : "Live", systemImage: model.muted ? "mic.slash.fill" : "mic.fill")
                .font(.headline)
                .foregroundStyle(model.muted ? .red : .green)
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
            DisclosureGroup("Custom color") {
                ColorWheel(model: model)
                    .frame(width: 150, height: 150)
                    .frame(maxWidth: .infinity)
            }
            HStack {
                Text("Brightness")
                Slider(value: $model.brightness, in: 0...100)
            }
            if let e = model.error {
                Text(e).font(.caption).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
            }
            Divider()
            Button("Quit MicDeck") { md_light_stop(); NSApp.terminate(nil) }
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
