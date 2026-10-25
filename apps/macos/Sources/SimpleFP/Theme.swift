import SwiftUI

enum Palette {
    /// 465 nm channel (sensor emission is green).
    static let signal = Color(red: 0.18, green: 0.72, blue: 0.47)
    /// 405 nm control.
    static let control = Color(red: 0.55, green: 0.42, blue: 0.95)
    static let baseline = Color.secondary
    /// Fixed (not accent) so data colour does not change with window focus.
    static let dff = Color(red: 0.24, green: 0.52, blue: 0.98)
    static let event = Color.orange
    static let significant = Color(red: 0.96, green: 0.36, blue: 0.30)
    static let trimmed = Color.primary.opacity(0.05)
    static let grid = Color.primary.opacity(0.07)

    static func events(_ i: Int) -> Color {
        let set: [Color] = [.orange, .pink, .teal, .yellow, .mint, .indigo, .brown]
        return set[i % set.count]
    }

    /// Diverging blue–white–red map (perceptually ordered, symmetric about 0).
    static func diverging(_ v: Float) -> (UInt8, UInt8, UInt8) {
        let x = max(-1, min(1, v))
        let stops: [(Float, Float, Float)] = [(0.13, 0.30, 0.62), (0.40, 0.62, 0.84), (0.97, 0.97, 0.97), (0.90, 0.48, 0.38), (0.70, 0.09, 0.17)]
        let p = (x + 1) / 2 * Float(stops.count - 1)
        let i = min(Int(p), stops.count - 2)
        let f = p - Float(i)
        let (a, b) = (stops[i], stops[i + 1])
        func m(_ u: Float, _ w: Float) -> UInt8 { UInt8(max(0, min(255, (u + (w - u) * f) * 255))) }
        return (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
    }
}

/// "Nice" axis ticks (1-2-5 steps).
func niceTicks(_ lo: Double, _ hi: Double, target: Int = 5) -> [Double] {
    guard hi > lo, lo.isFinite, hi.isFinite else { return [] }
    let raw = (hi - lo) / Double(max(target, 1))
    let mag = pow(10, floor(log10(raw)))
    let step = [1, 2, 5, 10].map { $0 * mag }.first { $0 >= raw } ?? 10 * mag
    var t = ceil(lo / step) * step
    var out: [Double] = []
    while t <= hi + step * 1e-9 {
        out.append(abs(t) < step * 1e-9 ? 0 : t)
        t += step
    }
    return out
}

func formatTick(_ v: Double, step: Double) -> String {
    let decimals = max(0, min(4, Int(-floor(log10(max(step, 1e-12))))))
    return String(format: "%.\(decimals)f", v)
}

func formatClock(_ s: Double) -> String {
    let tenths = Int((max(0, s) * 10).rounded())
    if tenths < 600 { return String(format: "%.1f s", Double(tenths) / 10) }
    let (m, r) = tenths.quotientAndRemainder(dividingBy: 600)
    return String(format: "%d:%04.1f", m, Double(r) / 10)
}
