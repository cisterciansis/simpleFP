import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// A cohort manifest and its group analysis (animal = unit).
@MainActor @Observable
final class CohortDoc: Identifiable {
    let id = UUID()
    let url: URL
    var results: CohortResults?
    var error: String?
    var isRunning = false
    var event: String?
    var metric = "mean"
    private var cohort: Cohort?

    init(url: URL) { self.url = url }

    var title: String { results?.name ?? url.deletingPathExtension().lastPathComponent }

    func run() async {
        isRunning = true
        defer { isRunning = false }
        let path = url.path
        do {
            let (c, r) = try await Task.detached(priority: .userInitiated) {
                let c = try Cohort.open(manifestPath: path)
                return (c, c.results())
            }.value
            cohort = c
            results = r
            error = nil
            if event == nil || !r.groups.contains(where: { $0.event == event }) { event = r.groups.first?.event }
        } catch {
            self.error = error.localizedDescription
        }
    }

    func export(to dir: URL) throws {
        guard let cohort else { return }
        _ = try cohort.export(dir: dir.path)
    }
}

extension UTType {
    static let toml = UTType(filenameExtension: "toml") ?? .plainText
}

// MARK: - Views

/// Stable colour per "group · condition" cell, in order of appearance.
private func cellColor(_ i: Int) -> Color {
    let set: [Color] = [Color(red: 0.24, green: 0.52, blue: 0.98), Color(red: 0.96, green: 0.45, blue: 0.20), Color(red: 0.18, green: 0.72, blue: 0.47), Color(red: 0.80, green: 0.30, blue: 0.70), .teal, .brown]
    return set[i % set.count]
}

struct CohortView: View {
    @Environment(Library.self) private var library
    let doc: CohortDoc

    var body: some View {
        @Bindable var doc = doc
        Group {
            if let r = doc.results {
                let event = doc.event ?? r.groups.first?.event ?? ""
                let groups = r.groups.filter { $0.event == event }
                let contrasts = r.contrasts.filter { $0.event == event }
                VSplitView {
                    VStack(alignment: .leading, spacing: 10) {
                        header(r, events: Array(Set(r.groups.map(\.event))).sorted())
                        HStack(spacing: 14) {
                            BandPlot(t: r.t, bands: groups, colors: groups.indices.map(cellColor), unit: r.unit, title: "Group mean ± 95% CI (animals)")
                            VStack(spacing: 8) {
                                ForEach(contrasts, id: \.label) { c in
                                    BandPlot(t: r.t, bands: [c], colors: [.secondary], unit: "Δ " + r.unit, title: "\(c.label) · n = \(c.n.map(String.init).joined(separator: "/"))")
                                }
                                if contrasts.isEmpty {
                                    ContentUnavailableView("No contrasts", systemImage: "arrow.left.arrow.right", description: Text("Need ≥ 3 animals per side."))
                                }
                            }
                        }
                        .frame(minHeight: 220)
                    }
                    .padding(12)
                    .frame(minHeight: 300)

                    HStack(alignment: .top, spacing: 14) {
                        VStack(alignment: .leading, spacing: 6) {
                            Picker("Metric", selection: $doc.metric) {
                                Text("Mean").tag("mean"); Text("AUC").tag("auc"); Text("Peak").tag("peak"); Text("Trough").tag("trough")
                            }
                            .pickerStyle(.segmented).labelsHidden().fixedSize()
                            EstimationPlot(r: r, event: event, metric: doc.metric, cells: groups)
                        }
                        .frame(minWidth: 220, maxWidth: .infinity)
                        TestsTable(rows: r.tests.filter { $0.event == event && $0.metric == doc.metric })
                            .frame(minWidth: 240, maxWidth: .infinity)
                    }
                    .padding(12)
                    .frame(minHeight: 220)
                }
            } else if let e = doc.error {
                ContentUnavailableView("Couldn’t analyze cohort", systemImage: "exclamationmark.triangle", description: Text(e))
            } else {
                ProgressView("Analyzing cohort…")
            }
        }
        .navigationTitle(doc.title)
        .navigationSubtitle(doc.results.map { "\($0.sessions.filter { $0.status == "ok" }.count)/\($0.sessions.count) sessions · \($0.presetName)" } ?? "")
        .toolbar {
            ToolbarItem {
                Button { library.exportCohort(doc) } label: { Label("Export", systemImage: "square.and.arrow.up") }
                    .disabled(doc.results == nil)
                    .help("Export group tables, per-session results and cohort.json")
            }
            ToolbarItem {
                Button { Task { await doc.run() } } label: { Label("Re-run", systemImage: "arrow.clockwise") }
                    .disabled(doc.isRunning)
                    .help("Re-read the manifest and re-run every session")
            }
        }
    }

    @ViewBuilder private func header(_ r: CohortResults, events: [String]) -> some View {
        @Bindable var doc = doc
        HStack(spacing: 10) {
            if events.count > 1 {
                Picker("Event", selection: $doc.event) { ForEach(events, id: \.self) { Text($0).tag(Optional($0)) } }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
            } else if let e = events.first {
                Text(e).font(.headline)
            }
            if !r.description.isEmpty { Text(r.description).font(.callout).foregroundStyle(.secondary).lineLimit(1).layoutPriority(-1) }
            Spacer()
            if !r.warnings.isEmpty {
                Label("\(r.warnings.count)", systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.yellow).font(.callout)
                    .help(r.warnings.joined(separator: "\n"))
            }
            Text("Two-stage: trials → animal means → group").font(.caption).foregroundStyle(.secondary).lineLimit(1).layoutPriority(-1)
                .help("Trials are averaged within animal × condition; group statistics treat each animal as one observation.")
        }
    }
}

/// Mean ± CI bands on the peri-event grid, with significance bars per band.
private struct BandPlot: View {
    let t: [Double]
    let bands: [CurveBand]
    let colors: [Color]
    let unit: String
    let title: String

    private func legend(_ font: Font) -> some View {
        HStack(spacing: 10) {
            ForEach(Array(bands.enumerated()), id: \.offset) { i, b in
                HStack(spacing: 4) {
                    Circle().fill(colors[i]).frame(width: 7, height: 7)
                    Text("\(b.label) (\(b.n.first ?? 0))").font(font).foregroundStyle(.secondary)
                }
            }
        }
        .fixedSize()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.caption.weight(.medium)).foregroundStyle(.secondary).lineLimit(1)
            if bands.count > 1 {
                // Legend shrinks instead of widening the window.
                ViewThatFits(in: .horizontal) {
                    legend(.caption2)
                    legend(.system(size: 9))
                    EmptyView()
                }
            }
            Canvas { ctx, size in
                guard t.count > 1 else { return }
                let rect = CGRect(x: 44, y: 4, width: max(1, size.width - 52), height: max(1, size.height - 22))
                let vals = bands.flatMap { $0.lo.isEmpty ? $0.mean : $0.lo + $0.hi }.filter(\.isFinite)
                var lo = min(vals.min() ?? 0, 0), hi = max(vals.max() ?? 1, 0)
                let pad = max(hi - lo, 1e-9) * 0.1
                lo -= pad; hi += pad
                let x = { (v: Double) in rect.minX + CGFloat((v - t[0]) / (t[t.count - 1] - t[0])) * rect.width }
                let y = { (v: Double) in rect.maxY - CGFloat((v - lo) / (hi - lo)) * rect.height }
                for v in niceTicks(lo, hi, target: 4) {
                    var p = Path(); p.move(to: CGPoint(x: rect.minX, y: y(v))); p.addLine(to: CGPoint(x: rect.maxX, y: y(v)))
                    ctx.stroke(p, with: .color(v == 0 ? .primary.opacity(0.25) : Palette.grid), lineWidth: v == 0 ? 1 : 0.5)
                    ctx.draw(Text(formatTick(v, step: (hi - lo) / 4)).font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary), at: CGPoint(x: rect.minX - 5, y: y(v)), anchor: .trailing)
                }
                for v in niceTicks(t[0], t[t.count - 1], target: 5) {
                    ctx.draw(Text(formatTick(v, step: 5)).font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary), at: CGPoint(x: x(v), y: rect.maxY + 4), anchor: .top)
                }
                var z = Path(); z.move(to: CGPoint(x: x(0), y: rect.minY)); z.addLine(to: CGPoint(x: x(0), y: rect.maxY))
                ctx.stroke(z, with: .color(.primary.opacity(0.35)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                for (i, b) in bands.enumerated() {
                    let c = colors[i]
                    if b.lo.count == t.count {
                        var band = Path()
                        band.move(to: CGPoint(x: x(t[0]), y: y(b.hi[0])))
                        for j in t.indices { band.addLine(to: CGPoint(x: x(t[j]), y: y(b.hi[j]))) }
                        for j in t.indices.reversed() { band.addLine(to: CGPoint(x: x(t[j]), y: y(b.lo[j]))) }
                        band.closeSubpath()
                        ctx.fill(band, with: .color(c.opacity(0.18)))
                    }
                    var line = Path()
                    for j in t.indices where j < b.mean.count {
                        let p = CGPoint(x: x(t[j]), y: y(b.mean[j]))
                        if j == 0 { line.move(to: p) } else { line.addLine(to: p) }
                    }
                    ctx.stroke(line, with: .color(c), style: StrokeStyle(lineWidth: 1.6, lineJoin: .round))
                    let dt = t[1] - t[0]
                    for j in b.significant.indices where b.significant[j] {
                        let r = CGRect(x: x(t[j] - dt / 2), y: rect.maxY - 3 - CGFloat(i) * 4, width: x(t[j] + dt / 2) - x(t[j] - dt / 2) + 0.5, height: 2.5)
                        ctx.fill(Path(r), with: .color(bands.count == 1 ? Palette.significant : c))
                    }
                }
                ctx.draw(Text(unit).font(.system(size: 10, weight: .medium)).foregroundStyle(.secondary), at: CGPoint(x: rect.minX + 4, y: rect.minY), anchor: .topLeading)
            }
        }
        .accessibilityLabel(title)
    }
}

/// Estimation plot: every animal as a dot, group mean with hierarchical-bootstrap CI,
/// and lines joining the same animal across conditions.
private struct EstimationPlot: View {
    let r: CohortResults
    let event: String
    let metric: String
    let cells: [CurveBand]

    private func value(_ a: AnimalPoint) -> Double {
        switch metric { case "auc": a.auc; case "peak": a.peak; case "trough": a.trough; default: a.mean }
    }

    var body: some View {
        Canvas { ctx, size in
            let cols = cells.map { ($0.group, $0.condition) }
            guard !cols.isEmpty else { return }
            let pts = r.animals.filter { $0.event == event }
            let sums = r.summaries.filter { $0.event == event && $0.metric == metric }
            let vals = pts.map(value) + sums.flatMap { [$0.lo, $0.hi] }.filter(\.isFinite)
            var lo = min(vals.min() ?? 0, 0), hi = max(vals.max() ?? 1, 0)
            let pad = max(hi - lo, 1e-9) * 0.1
            lo -= pad; hi += pad
            let rect = CGRect(x: 44, y: 6, width: max(1, size.width - 52), height: max(1, size.height - 40))
            let colW = rect.width / CGFloat(cols.count)
            let cx = { (i: Int) in rect.minX + colW * (CGFloat(i) + 0.5) }
            let y = { (v: Double) in rect.maxY - CGFloat((v - lo) / (hi - lo)) * rect.height }
            for v in niceTicks(lo, hi, target: 4) {
                var p = Path(); p.move(to: CGPoint(x: rect.minX, y: y(v))); p.addLine(to: CGPoint(x: rect.maxX, y: y(v)))
                ctx.stroke(p, with: .color(v == 0 ? .primary.opacity(0.25) : Palette.grid), lineWidth: v == 0 ? 1 : 0.5)
                ctx.draw(Text(formatTick(v, step: (hi - lo) / 4)).font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary), at: CGPoint(x: rect.minX - 5, y: y(v)), anchor: .trailing)
            }
            // Deterministic jitter by animal name keeps dots still across redraws.
            func jitter(_ s: String) -> CGFloat { CGFloat(s.unicodeScalars.reduce(7) { ($0 &* 31 &+ Int($1.value)) % 1000 }) / 1000 * 0.5 - 0.25 }
            var pos: [String: [(Int, CGPoint)]] = [:]
            for (i, c) in cols.enumerated() {
                for a in pts where a.group == c.0 && a.condition == c.1 {
                    let p = CGPoint(x: cx(i) + jitter(a.animal) * colW * 0.5, y: y(value(a)))
                    pos[a.animal, default: []].append((i, p))
                }
            }
            for (_, ps) in pos where ps.count > 1 {
                var path = Path()
                for (k, (_, p)) in ps.sorted(by: { $0.0 < $1.0 }).enumerated() { if k == 0 { path.move(to: p) } else { path.addLine(to: p) } }
                ctx.stroke(path, with: .color(.secondary.opacity(0.35)), lineWidth: 0.8)
            }
            for (i, c) in cols.enumerated() {
                for (_, ps) in pos { for (j, p) in ps where j == i { ctx.fill(Path(ellipseIn: CGRect(x: p.x - 3.5, y: p.y - 3.5, width: 7, height: 7)), with: .color(cellColor(i).opacity(0.85))) } }
                if let s = sums.first(where: { $0.group == c.0 && $0.condition == c.1 }) {
                    let x = cx(i) + colW * 0.3
                    if s.lo.isFinite {
                        var bar = Path(); bar.move(to: CGPoint(x: x, y: y(s.lo))); bar.addLine(to: CGPoint(x: x, y: y(s.hi)))
                        ctx.stroke(bar, with: .color(.primary), lineWidth: 1.5)
                    }
                    ctx.fill(Path(ellipseIn: CGRect(x: x - 4.5, y: y(s.mean) - 4.5, width: 9, height: 9)), with: .color(.primary))
                }
                ctx.draw(Text("\(c.0)\n\(c.1)").font(.system(size: 10)).foregroundStyle(.secondary), at: CGPoint(x: cx(i), y: rect.maxY + 4), anchor: .top)
            }
        }
        .accessibilityLabel("Per-animal \(metric) by group and condition")
    }
}

private struct TestRow: Identifiable {
    let id: Int
    let t: MetricTestRow
}

private struct TestsTable: View {
    let rows: [MetricTestRow]

    var body: some View {
        Table(rows.enumerated().map { TestRow(id: $0.offset, t: $0.element) }) {
            TableColumn("Contrast") { Text($0.t.contrast).help($0.t.contrast) }.width(min: 90, ideal: 150)
            TableColumn("Δ [95% CI]") { r in Text(String(format: "%+.3f [%+.3f, %+.3f]", r.t.estimate, r.t.lo, r.t.hi)).monospacedDigit() }.width(min: 150, ideal: 180)
            TableColumn("p") { r in Text(String(format: "%.4f", r.t.p) + (r.t.exact ? "" : "*")).monospacedDigit().help(r.t.exact ? "Exact permutation p" : "Monte-Carlo permutation p (20 000)") }.width(min: 46, ideal: 56)
            TableColumn("q (BH)") { r in Text(String(format: "%.4f", r.t.q)).monospacedDigit() }.width(min: 46, ideal: 56)
            TableColumn("n") { r in Text("\(r.t.nA)/\(r.t.nB)").monospacedDigit() }.width(min: 34, ideal: 40)
        }
        .tableStyle(.bordered(alternatesRowBackgrounds: true))
        .font(.callout)
        .overlay { if rows.isEmpty { Text("No tests (need ≥ 3 animals per side)").foregroundStyle(.secondary) } }
    }
}

/// Assign animal / group / condition to open recordings and save a cohort manifest.
struct NewCohortSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var name = "New cohort"
    @State private var rows: [Assignment] = []
    @State private var error: String?

    struct Assignment: Identifiable {
        let id: UUID
        let url: URL
        var animal: String
        var group: String
        var condition: String
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("New cohort").font(.title3.weight(.semibold))
            Text("Each recording needs an animal; animals keep one group. Statistics treat each animal as one observation.")
                .font(.callout).foregroundStyle(.secondary)
            TextField("Name", text: $name).textFieldStyle(.roundedBorder)
            Table($rows) {
                TableColumn("Recording") { $r in Text(r.url.lastPathComponent).lineLimit(1).truncationMode(.middle) }
                TableColumn("Animal") { $r in TextField("Animal", text: $r.animal) }
                TableColumn("Group") { $r in TextField("Group", text: $r.group) }
                TableColumn("Condition") { $r in TextField("Condition", text: $r.condition) }
            }
            .frame(minHeight: 220)
            if let error { Label(error, systemImage: "xmark.octagon.fill").foregroundStyle(.red).font(.callout) }
            HStack {
                Text("Events: \(sharedEpocs.isEmpty ? "none shared" : sharedEpocs.joined(separator: ", "))").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save & Analyze…") { save() }.keyboardShortcut(.defaultAction).disabled(rows.isEmpty || sharedEpocs.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 720, height: 520)
        .onAppear {
            // Guess the animal from the block name ("A1-250619-103950" → "A1").
            rows = library.recordings.map { r in
                Assignment(id: r.id, url: r.url, animal: String(r.title.split(separator: "-").first ?? Substring(r.title)), group: "all", condition: "session")
            }
        }
    }

    private var sharedEpocs: [String] {
        let sets = library.recordings.map(\.epocs)
        guard let first = sets.first else { return [] }
        return sets.dropFirst().reduce(first) { $0.intersection($1) }.sorted()
    }

    private func save() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.toml]
        panel.nameFieldStringValue = "cohort.toml"
        panel.message = "Save the cohort manifest (block paths are stored relative to it when possible)"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let base = url.deletingLastPathComponent().standardizedFileURL.path + "/"
        let sessions = rows.map { r in
            let p = r.url.standardizedFileURL.path
            return SessionRow(block: p.hasPrefix(base) ? String(p.dropFirst(base.count)) : p, animal: r.animal, group: r.group, condition: r.condition, status: "")
        }
        do {
            _ = try writeCohortManifest(path: url.path, name: name, presetId: library.recordings.first?.preset.id ?? "grab-ach3.0-tdt", epocs: sharedEpocs, sessions: sessions)
            dismiss()
            Task { await library.open(url) }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct CohortInspector: View {
    let doc: CohortDoc

    var body: some View {
        Form {
            if let r = doc.results {
                Section("Sessions") {
                    ForEach(r.sessions, id: \.block) { s in
                        LabeledContent {
                            Text(s.status == "ok" ? "\(s.group) · \(s.condition)" : s.status)
                                .foregroundStyle(s.status == "ok" ? Color.secondary : Color.orange)
                                .lineLimit(2)
                        } label: {
                            Text("\(s.animal)  ").fontWeight(.medium) + Text(s.block).foregroundStyle(.secondary)
                        }
                    }
                }
                if !r.warnings.isEmpty {
                    Section("Warnings") {
                        ForEach(r.warnings, id: \.self) { Label($0, systemImage: "exclamationmark.triangle").foregroundStyle(.orange) }
                    }
                }
                Section("Method") {
                    Text("Trials are averaged within each animal × condition. Curves: bootstrap 95% CI across animals (expanded √(n/(n−1))), runs ≥ 1/low-pass s. Metrics: hierarchical bootstrap CIs; permutation tests (exact when feasible); Benjamini–Hochberg q across all tests.")
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
            Section("Manifest") {
                Text(doc.url.path).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([doc.url]) }
            }
        }
        .formStyle(.grouped)
    }
}
