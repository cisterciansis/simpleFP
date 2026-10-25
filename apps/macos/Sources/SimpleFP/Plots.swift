import AppKit
import SwiftUI

enum TraceMode: String, CaseIterable, Identifiable {
    case dff = "ΔF/F", z = "z-score", raw = "Raw"
    var id: Self { self }
}

private struct Series {
    let kind: TraceKind
    let color: Color
    let width: CGFloat
    var dashed = false
}

private struct Envelope {
    let t: [Float], lo: [Float], hi: [Float]
    init(_ d: Data) {
        let f = d.floats()
        let n = f.count / 3
        var t = [Float](), lo = [Float](), hi = [Float]()
        t.reserveCapacity(n); lo.reserveCapacity(n); hi.reserveCapacity(n)
        for i in 0..<n { t.append(f[3 * i]); lo.append(f[3 * i + 1]); hi.append(f[3 * i + 2]) }
        self.t = t; self.lo = lo; self.hi = hi
    }
    var bounds: (Double, Double)? {
        guard let a = lo.min(), let b = hi.max() else { return nil }
        return (Double(a), Double(b))
    }
}

private let inset = EdgeInsets(top: 10, leading: 52, bottom: 24, trailing: 14)

/// Maps data space to view space inside the plot inset.
private struct Frame {
    let x0: Double, x1: Double, y0: Double, y1: Double
    let rect: CGRect
    func x(_ t: Double) -> CGFloat { rect.minX + CGFloat((t - x0) / (x1 - x0)) * rect.width }
    func y(_ v: Double) -> CGFloat { rect.maxY - CGFloat((v - y0) / (y1 - y0)) * rect.height }
}

private func plotRect(_ size: CGSize) -> CGRect {
    CGRect(x: inset.leading, y: inset.top, width: max(1, size.width - inset.leading - inset.trailing), height: max(1, size.height - inset.top - inset.bottom))
}

private func drawAxes(_ ctx: inout GraphicsContext, _ f: Frame, yLabel: String) {
    let ys = niceTicks(f.y0, f.y1, target: 5)
    let ystep = ys.count > 1 ? ys[1] - ys[0] : 1
    for v in ys {
        var p = Path(); p.move(to: CGPoint(x: f.rect.minX, y: f.y(v))); p.addLine(to: CGPoint(x: f.rect.maxX, y: f.y(v)))
        ctx.stroke(p, with: .color(v == 0 ? Color.primary.opacity(0.22) : Palette.grid), lineWidth: v == 0 ? 1 : 0.5)
        ctx.draw(Text(formatTick(v, step: ystep)).font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary), at: CGPoint(x: f.rect.minX - 6, y: f.y(v)), anchor: .trailing)
    }
    let xs = niceTicks(f.x0, f.x1, target: max(3, Int(f.rect.width / 110)))
    let xstep = xs.count > 1 ? xs[1] - xs[0] : 1
    for v in xs {
        var p = Path(); p.move(to: CGPoint(x: f.x(v), y: f.rect.minY)); p.addLine(to: CGPoint(x: f.x(v), y: f.rect.maxY))
        ctx.stroke(p, with: .color(Palette.grid), lineWidth: 0.5)
        ctx.draw(Text(formatTick(v, step: xstep)).font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary), at: CGPoint(x: f.x(v), y: f.rect.maxY + 5), anchor: .top)
    }
    ctx.draw(Text(yLabel).font(.system(size: 10, weight: .medium)).foregroundStyle(.secondary), at: CGPoint(x: f.rect.minX + 4, y: f.rect.minY), anchor: .topLeading)
}

private func envelopePath(_ e: Envelope, _ f: Frame) -> Path {
    var p = Path()
    for i in e.t.indices {
        let x = f.x(Double(e.t[i]))
        let a = CGPoint(x: x, y: f.y(Double(e.lo[i]))), b = CGPoint(x: x, y: f.y(Double(e.hi[i])))
        if i == 0 { p.move(to: a) } else { p.addLine(to: a) }
        if e.lo[i] != e.hi[i] { p.addLine(to: b) }
    }
    return p
}

/// Session trace with min/max level-of-detail from the engine; pan, pinch and scroll to navigate.
struct TracePlot: View {
    let rec: Recording
    let mode: TraceMode
    @State private var dragStart: ClosedRange<Double>?
    @State private var zoomStart: ClosedRange<Double>?
    @State private var hover: CGPoint?

    private var series: [Series] {
        switch mode {
        case .dff: [Series(kind: .dff, color: Palette.dff, width: 1.1)]
        case .z: [Series(kind: .z, color: Palette.dff, width: 1.1)]
        case .raw: [
            Series(kind: .signalBaseline, color: Palette.baseline, width: 1, dashed: true),
            Series(kind: .fittedControl, color: Palette.control, width: 1),
            Series(kind: .signal, color: Palette.signal, width: 1.1),
        ]
        }
    }

    private var yLabel: String {
        switch mode { case .dff: "ΔF/F (%)"; case .z: "z (session)"; case .raw: "\(rec.info.signal) · fitted \(rec.info.control ?? "control") (a.u.)" }
    }

    var body: some View {
        let session = rec.session, range = rec.visible, series = series, revision = rec.revision
        let qc = rec.results?.qc, events = rec.results?.events ?? []
        GeometryReader { geo in
            Canvas { ctx, size in
                _ = revision
                let rect = plotRect(size)
                let buckets = UInt32(rect.width * 2)
                let envs = series.map { Envelope(session.envelope(kind: $0.kind, t0: range.lowerBound, t1: range.upperBound, buckets: buckets)) }
                let bounds = envs.compactMap(\.bounds)
                guard var lo = bounds.map(\.0).min(), var hi = bounds.map(\.1).max() else { return }
                if hi - lo < 1e-9 { lo -= 1; hi += 1 }
                let pad = (hi - lo) * 0.08
                let f = Frame(x0: range.lowerBound, x1: range.upperBound, y0: lo - pad, y1: hi + pad, rect: rect)

                if let qc {
                    for (a, b) in [(range.lowerBound, qc.analyzedFromS), (qc.analyzedToS, range.upperBound)] where b > a {
                        let r = CGRect(x: f.x(max(a, range.lowerBound)), y: rect.minY, width: f.x(min(b, range.upperBound)) - f.x(max(a, range.lowerBound)), height: rect.height)
                        if r.width > 0 { ctx.fill(Path(r), with: .color(Palette.trimmed)) }
                    }
                }
                drawAxes(&ctx, f, yLabel: yLabel)
                for (i, ev) in events.enumerated() {
                    for t in ev.eventTimes + ev.dropped where range.contains(t) {
                        var p = Path(); p.move(to: CGPoint(x: f.x(t), y: rect.minY)); p.addLine(to: CGPoint(x: f.x(t), y: rect.maxY))
                        ctx.stroke(p, with: .color(Palette.events(i).opacity(ev.dropped.contains(t) ? 0.25 : 0.55)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                    }
                }
                ctx.clip(to: Path(rect))
                for (s, e) in zip(series, envs) {
                    ctx.stroke(envelopePath(e, f), with: .color(s.color), style: StrokeStyle(lineWidth: s.width, lineJoin: .round, dash: s.dashed ? [4, 3] : []))
                }
            }
            .overlay(alignment: .topTrailing) { legend.padding(.trailing, inset.trailing + 6).padding(.top, inset.top + 4) }
            .contentShape(Rectangle())
            .gesture(pan(width: plotRect(geo.size).width))
            .simultaneousGesture(zoom)
            .onTapGesture(count: 2) { withAnimation(.snappy) { rec.visible = 0...max(rec.info.durationS, 1) } }
            .onContinuousHover { phase in
                if case .active(let p) = phase { hover = p } else { hover = nil }
            }
            .background(ScrollWheelCatcher { dx, dy, zoomMod in
                guard let hover else { return false }
                let rect = plotRect(geo.size)
                let anchor = Double((hover.x - rect.minX) / rect.width)
                if zoomMod || abs(dy) > abs(dx) * 1.5 {
                    zoomBy(exp(Double(-dy) * 0.01), anchor: anchor)
                } else {
                    shift(by: Double(-dx) / Double(rect.width) * span)
                }
                return true
            })
        }
    }

    private var legend: some View {
        HStack(spacing: 12) {
            if mode == .raw {
                Label(rec.info.signal, systemImage: "circle.fill").foregroundStyle(Palette.signal)
                Label("fitted \(rec.info.control ?? "control")", systemImage: "circle.fill").foregroundStyle(Palette.control)
                Label("bleaching", systemImage: "circle.dashed").foregroundStyle(.secondary)
            }
            Text(formatClock(span)).foregroundStyle(.secondary)
        }
        .labelStyle(CompactLabel())
        .font(.system(size: 10, weight: .medium))
        .padding(.horizontal, 8).padding(.vertical, 4)
        .background(.regularMaterial, in: Capsule())
    }

    private var span: Double { rec.visible.upperBound - rec.visible.lowerBound }

    private func clamp(_ a: Double, _ b: Double) -> ClosedRange<Double> {
        let total = max(rec.info.durationS, 1)
        let w = min(max(b - a, 0.5), total)
        let lo = min(max(a, 0), total - w)
        return lo...(lo + w)
    }

    private func shift(by dt: Double) {
        rec.visible = clamp(rec.visible.lowerBound + dt, rec.visible.upperBound + dt)
    }

    private func zoomBy(_ k: Double, anchor: Double) {
        let a = rec.visible.lowerBound + anchor * span
        rec.visible = clamp(a - (a - rec.visible.lowerBound) * k, a + (rec.visible.upperBound - a) * k)
    }

    private func pan(width: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 2)
            .onChanged { v in
                let start = dragStart ?? rec.visible
                dragStart = start
                let dt = -Double(v.translation.width / width) * (start.upperBound - start.lowerBound)
                rec.visible = clamp(start.lowerBound + dt, start.upperBound + dt)
            }
            .onEnded { _ in dragStart = nil }
    }

    private var zoom: some Gesture {
        MagnifyGesture()
            .onChanged { v in
                let start = zoomStart ?? rec.visible
                zoomStart = start
                let mid = (start.lowerBound + start.upperBound) / 2, half = (start.upperBound - start.lowerBound) / 2 / v.magnification
                rec.visible = clamp(mid - half, mid + half)
            }
            .onEnded { _ in zoomStart = nil }
    }
}

private struct CompactLabel: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) { configuration.icon.imageScale(.small); configuration.title.foregroundStyle(.secondary) }
    }
}

/// Whole-session miniature with a draggable window showing the visible range.
struct OverviewStrip: View {
    let rec: Recording

    var body: some View {
        let session = rec.session, revision = rec.revision, total = max(rec.info.durationS, 1), vis = rec.visible
        GeometryReader { geo in
            Canvas { ctx, size in
                _ = revision
                let rect = CGRect(x: inset.leading, y: 2, width: max(1, size.width - inset.leading - inset.trailing), height: size.height - 4)
                let e = Envelope(session.envelope(kind: .dff, t0: 0, t1: total, buckets: UInt32(rect.width)))
                guard let (lo, hi) = e.bounds, hi > lo else { return }
                let f = Frame(x0: 0, x1: total, y0: lo, y1: hi, rect: rect)
                ctx.fill(Path(roundedRect: rect, cornerRadius: 5), with: .color(Color.primary.opacity(0.035)))
                ctx.stroke(envelopePath(e, f), with: .color(Color.secondary.opacity(0.7)), lineWidth: 0.8)
                let w = CGRect(x: f.x(vis.lowerBound), y: rect.minY, width: max(3, f.x(vis.upperBound) - f.x(vis.lowerBound)), height: rect.height)
                ctx.fill(Path(roundedRect: w, cornerRadius: 4), with: .color(Color.accentColor.opacity(0.16)))
                ctx.stroke(Path(roundedRect: w, cornerRadius: 4), with: .color(Color.accentColor.opacity(0.8)), lineWidth: 1)
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { v in
                let width = max(1, geo.size.width - inset.leading - inset.trailing)
                let c = Double((v.location.x - inset.leading) / width) * total
                let half = (vis.upperBound - vis.lowerBound) / 2
                let lo = min(max(c - half, 0), total - 2 * half)
                rec.visible = lo...(lo + 2 * half)
            })
        }
        .frame(height: 34)
        .accessibilityLabel("Session overview")
    }
}

/// Peri-event mean with bootstrap CI and significance bar.
struct PethPlot: View {
    let ev: EventSummary
    let color: Color

    var body: some View {
        Canvas { ctx, size in
            guard ev.t.count > 1, !ev.mean.isEmpty else { return }
            let rect = plotRect(size).insetBy(dx: 0, dy: 0)
            let lo = min((ev.lo.isEmpty ? ev.mean : ev.lo).min() ?? 0, ev.controlLo.min() ?? 0)
            let hi = max((ev.hi.isEmpty ? ev.mean : ev.hi).max() ?? 1, ev.controlHi.max() ?? 0)
            let pad = max(hi - lo, 1e-9) * 0.12
            let f = Frame(x0: ev.t.first!, x1: ev.t.last!, y0: min(lo, 0) - pad, y1: max(hi, 0) + pad, rect: rect)
            drawAxes(&ctx, f, yLabel: ev.unit)
            if !ev.lo.isEmpty {
                var band = Path()
                band.move(to: CGPoint(x: f.x(ev.t[0]), y: f.y(ev.hi[0])))
                for i in ev.t.indices { band.addLine(to: CGPoint(x: f.x(ev.t[i]), y: f.y(ev.hi[i]))) }
                for i in ev.t.indices.reversed() { band.addLine(to: CGPoint(x: f.x(ev.t[i]), y: f.y(ev.lo[i]))) }
                band.closeSubpath()
                ctx.fill(band, with: .color(color.opacity(0.22)))
            }
            if ev.controlMean.count == ev.t.count {
                var c = Path()
                for i in ev.t.indices {
                    let p = CGPoint(x: f.x(ev.t[i]), y: f.y(ev.controlMean[i]))
                    if i == 0 { c.move(to: p) } else { c.addLine(to: p) }
                }
                ctx.stroke(c, with: .color(Palette.control.opacity(0.85)), style: StrokeStyle(lineWidth: 1.2, lineJoin: .round, dash: [4, 2]))
            }
            var zero = Path(); zero.move(to: CGPoint(x: f.x(0), y: rect.minY)); zero.addLine(to: CGPoint(x: f.x(0), y: rect.maxY))
            ctx.stroke(zero, with: .color(.primary.opacity(0.35)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
            var line = Path()
            for i in ev.t.indices {
                let p = CGPoint(x: f.x(ev.t[i]), y: f.y(ev.mean[i]))
                if i == 0 { line.move(to: p) } else { line.addLine(to: p) }
            }
            ctx.stroke(line, with: .color(color), style: StrokeStyle(lineWidth: 1.8, lineJoin: .round))
            let dt = ev.t[1] - ev.t[0]
            for i in ev.significant.indices where ev.significant[i] {
                let r = CGRect(x: f.x(ev.t[i] - dt / 2), y: rect.maxY - 4, width: f.x(ev.t[i] + dt / 2) - f.x(ev.t[i] - dt / 2) + 0.5, height: 3)
                ctx.fill(Path(r), with: .color(Palette.significant))
            }
        }
        .accessibilityLabel("Mean response to \(ev.label) with confidence interval")
    }
}

/// Trials × time heatmap on a symmetric diverging scale.
struct EventHeatmap: View {
    let ev: EventSummary

    private var image: (CGImage, Float)? {
        let v = ev.heatmap.floats()
        let cols = ev.t.count, rows = cols > 0 ? v.count / cols : 0
        guard rows > 0 else { return nil }
        let mags = v.filter(\.isFinite).map(abs).sorted()
        let lim = max(mags.isEmpty ? 1 : mags[min(mags.count - 1, Int(Double(mags.count) * 0.98))], 1e-6)
        var px = [UInt8](repeating: 0, count: rows * cols * 4)
        for i in 0..<(rows * cols) {
            let (r, g, b) = v[i].isFinite ? Palette.diverging(v[i] / lim) : (128, 128, 128)
            px[4 * i] = r; px[4 * i + 1] = g; px[4 * i + 2] = b; px[4 * i + 3] = 255
        }
        guard let provider = CGDataProvider(data: Data(px) as CFData),
              let img = CGImage(width: cols, height: rows, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: cols * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { return nil }
        return (img, lim)
    }

    var body: some View {
        if let (img, lim) = image {
            VStack(alignment: .leading, spacing: 4) {
                GeometryReader { geo in
                    let rect = plotRect(geo.size)
                    ZStack(alignment: .topLeading) {
                        Image(decorative: img, scale: 1).resizable().interpolation(.none)
                            .frame(width: rect.width, height: rect.height).offset(x: rect.minX, y: rect.minY)
                            .clipShape(RoundedRectangle(cornerRadius: 3))
                        Canvas { ctx, size in
                            let r = plotRect(size)
                            let f = Frame(x0: ev.t.first ?? 0, x1: ev.t.last ?? 1, y0: 0, y1: 1, rect: r)
                            var p = Path(); p.move(to: CGPoint(x: f.x(0), y: r.minY)); p.addLine(to: CGPoint(x: f.x(0), y: r.maxY))
                            ctx.stroke(p, with: .color(.primary.opacity(0.6)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                            let xs = niceTicks(f.x0, f.x1, target: 4)
                            let step = xs.count > 1 ? xs[1] - xs[0] : 1
                            for v in xs { ctx.draw(Text(formatTick(v, step: step)).font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary), at: CGPoint(x: f.x(v), y: r.maxY + 5), anchor: .top) }
                            ctx.draw(Text("trial 1").font(.system(size: 10)).foregroundStyle(.secondary), at: CGPoint(x: r.minX - 6, y: r.minY + 6), anchor: .trailing)
                            ctx.draw(Text("\(ev.eventTimes.count)").font(.system(size: 10)).foregroundStyle(.secondary), at: CGPoint(x: r.minX - 6, y: r.maxY - 6), anchor: .trailing)
                        }
                    }
                }
                HStack(spacing: 6) {
                    Text(formatTick(-Double(lim), step: Double(lim) / 2)).font(.system(size: 10).monospacedDigit())
                    LinearGradient(colors: stride(from: -1.0, through: 1.0, by: 0.25).map { v in
                        let (r, g, b) = Palette.diverging(Float(v)); return Color(red: Double(r) / 255, green: Double(g) / 255, blue: Double(b) / 255)
                    }, startPoint: .leading, endPoint: .trailing).frame(width: 90, height: 6).clipShape(Capsule())
                    Text(formatTick(Double(lim), step: Double(lim) / 2)).font(.system(size: 10).monospacedDigit())
                    Text(ev.unit).font(.system(size: 10)).foregroundStyle(.secondary)
                }
                .foregroundStyle(.secondary)
                .padding(.leading, inset.leading)
            }
            .accessibilityLabel("Heatmap of \(ev.eventTimes.count) trials for \(ev.label)")
        } else {
            ContentUnavailableView("No trials", systemImage: "square.grid.3x3")
        }
    }
}

/// Forwards trackpad/mouse scroll to SwiftUI while the pointer hovers the view.
private struct ScrollWheelCatcher: NSViewRepresentable {
    /// (dx, dy, zoomModifier) → consumed
    let handler: @MainActor (CGFloat, CGFloat, Bool) -> Bool

    final class Coordinator {
        var monitor: Any?
        var handler: (@MainActor (CGFloat, CGFloat, Bool) -> Bool)?
        weak var view: NSView?
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSView {
        let v = NSView()
        let c = context.coordinator
        c.view = v
        c.handler = handler
        c.monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak c] e in
            let k: CGFloat = e.hasPreciseScrollingDeltas ? 1 : 8
            let (dx, dy, cmd, loc, win) = (e.scrollingDeltaX * k, e.scrollingDeltaY * k, e.modifierFlags.contains(.command), e.locationInWindow, e.windowNumber)
            let consumed = MainActor.assumeIsolated { () -> Bool in
                guard let c, let v = c.view, let w = v.window, w.windowNumber == win, let h = c.handler else { return false }
                return v.bounds.contains(v.convert(loc, from: nil)) && h(dx, dy, cmd)
            }
            return consumed ? nil : e
        }
        return v
    }

    func updateNSView(_ v: NSView, context: Context) { context.coordinator.handler = handler }

    static func dismantleNSView(_ v: NSView, coordinator: Coordinator) {
        if let m = coordinator.monitor { NSEvent.removeMonitor(m) }
    }
}
