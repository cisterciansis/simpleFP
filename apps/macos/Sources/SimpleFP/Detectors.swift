import AppKit
import Charts
import SwiftUI

/// Trained behavior detectors stored as JSON model files (with their validation).
enum ModelLibrary {
    static var dir: URL {
        let d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("SimpleFP/models", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }

    struct Entry: Identifiable, Hashable {
        let id: String, behavior: String, url: URL, summary: String
    }

    static func all() -> [Entry] {
        let files = (try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
        return files.filter { $0.pathExtension == "json" }.compactMap { u in
            guard let d = try? Data(contentsOf: u), let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any],
                  let id = o["id"] as? String, let b = o["behavior"] as? String else { return nil }
            return Entry(id: id, behavior: b, url: u, summary: summary(o["evaluation"] as? [String: Any]))
        }
        .sorted { $0.id < $1.id }
    }

    static func summary(_ e: [String: Any]?) -> String {
        guard let e, let frame = e["frame"] as? [String: Any], let bout = e["bout"] as? [String: Any] else { return "" }
        let k = (frame["kappa"] as? Double) ?? .nan, f1 = (bout["f1"] as? Double) ?? .nan
        return String(format: "κ %.2f · bout F1 %.2f · %@", k, f1, (e["scheme"] as? String) ?? "")
    }

    static func save(_ json: String) throws -> URL {
        guard let o = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any], let id = o["id"] as? String else { throw NSError(domain: "SimpleFP", code: 30) }
        let url = dir.appendingPathComponent(id.replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "@", with: "_") + ".json")
        try json.write(to: url, atomically: true, encoding: .utf8)
        return url
    }
}

/// How trustworthy cross-validated agreement is, in plain words (Landis & Koch bands for κ).
private func quality(_ kappa: Double) -> (String, Color) {
    switch kappa {
    case 0.8...: ("Excellent agreement with your scoring", .green)
    case 0.6..<0.8: ("Good: review proposals carefully", .yellow)
    case 0.4..<0.6: ("Moderate: score more bouts before relying on it", .orange)
    default: ("Not reliable yet: score more (and more varied) bouts", .red)
    }
}

struct DetectorBar: View {
    @Bindable var model: VideoModel
    @State private var training = false
    @State private var models = ModelLibrary.all()

    var body: some View {
        HStack(spacing: 8) {
            Menu {
                if models.isEmpty { Text("No detectors yet: score bouts, track, then Train") }
                ForEach(models) { m in
                    Button("\(m.behavior) — \(m.summary)") { detect(m) }
                }
                Divider()
                Button("Train Detector…") { training = true }.disabled(model.tracker == nil)
                Button("Run Pose Model…") { pickPose() }.disabled(model.tracker == nil)
                Button("Show Detector Files") { NSWorkspace.shared.activateFileViewerSelecting([ModelLibrary.dir]) }
            } label: { Label("Detect", systemImage: "wand.and.stars") }
                .menuStyle(.borderlessButton).fixedSize()
                .disabled(model.tracker == nil)
                .help(model.tracker == nil ? "Track the animal first" : "Run a trained detector; its bouts arrive as proposals to review")
                .onAppear { models = ModelLibrary.all() }
            if model.poseBusy { ProgressView().controlSize(.small); Text("Running pose model…").font(.caption).foregroundStyle(.secondary) }
            if let p = model.pose { Text("pose: \(p.bodyparts.joined(separator: ", "))").font(.caption).foregroundStyle(.secondary) }
            Spacer()
        }
        .controlSize(.small)
        .sheet(isPresented: $training, onDismiss: { models = ModelLibrary.all() }) { TrainSheet(model: model) }
    }

    /// Pick a Core ML pose model (.mlpackage/.mlmodel/.mlmodelc) with its `.pose.json` sidecar.
    private func pickPose() {
        let panel = NSOpenPanel()
        panel.message = "Choose a Core ML pose model (needs a .pose.json sidecar). SimpleFP ships no pose weights; use models you are licensed to run."
        panel.canChooseDirectories = true // .mlpackage and .mlmodelc are directories
        panel.treatsFilePackagesAsDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await model.runPose(url) }
    }

    private func detect(_ m: ModelLibrary.Entry) {
        guard let tracker = model.tracker, let rec = model.recording, let json = try? String(contentsOf: m.url, encoding: .utf8) else { return }
        do {
            let out = try proposeBehavior(modelJson: json, session: rec.session, tracker: tracker, camera: model.camera)
            let props = try JSONDecoder().decode([AnnotationJSON].self, from: Data(out.utf8))
            // Replace earlier proposals from the same model that were never reviewed.
            model.annotations.removeAll { $0.model == m.id && $0.review == "proposed" }
            model.annotations += props
            model.annotations.sort { $0.startS < $1.startS }
            model.dirty = true
        } catch {
            model.error = error.localizedDescription
        }
    }
}

/// Human-verified bouts of a behavior: manual scoring plus model proposals a person accepted or edited.
@MainActor private func bouts(_ b: String, in v: VideoModel) -> [Double] {
    v.annotations.filter { a in
        a.label == b && a.endS != nil && (a.source != "model" ? a.review != "rejected" : ["accepted", "edited"].contains(a.review))
    }.flatMap { [$0.startS, $0.endS!] }
}

struct TrainSheet: View {
    @Bindable var model: VideoModel
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var behavior = ""
    @State private var excluded: Set<UUID> = []
    @State private var result: [String: Any]?
    @State private var json: String?
    @State private var busy = false
    @State private var error: String?

    /// Every open, tracked session (this one first); the user can leave any out.
    private var sources: [(rec: Recording, video: VideoModel)] {
        let all = library.recordings.compactMap { r in r.video.flatMap { $0.tracker == nil ? nil : (r, $0) } }
        return all.filter { $0.1 === model } + all.filter { $0.1 !== model }
    }
    private var included: [(rec: Recording, video: VideoModel)] { sources.filter { !excluded.contains($0.rec.id) && !bouts(behavior, in: $0.video).isEmpty } }
    private var boutCount: Int { included.reduce(0) { $0 + bouts(behavior, in: $1.video).count / 2 } }

    var body: some View {
        let candidates = model.ethogram.behavior.filter { $0.kind == "state" }.map(\.label)
        VStack(alignment: .leading, spacing: 12) {
            Text("Train a behavior detector").font(.title3.weight(.semibold))
            Text("Learns from human-verified bouts (your scoring and proposals you accepted) and the tracked kinematics. Accuracy is measured on sessions, or time blocks, the model never saw.")
                .font(.callout).foregroundStyle(.secondary)
            Picker("Behavior", selection: $behavior) {
                ForEach(candidates, id: \.self) { b in Text(b.replacingOccurrences(of: "_", with: " ")).tag(b) }
            }
            .onChange(of: behavior) { result = nil; json = nil }
            VStack(alignment: .leading, spacing: 2) {
                Text("Sessions").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                ForEach(sources, id: \.rec.id) { s in
                    let n = bouts(behavior, in: s.video).count / 2
                    Toggle(isOn: Binding(get: { !excluded.contains(s.rec.id) }, set: { on in if on { excluded.remove(s.rec.id) } else { excluded.insert(s.rec.id) }; result = nil; json = nil })) {
                        Text("\(s.rec.title) · \(n) bouts").foregroundStyle(n == 0 ? .secondary : .primary)
                    }
                    .disabled(n == 0)
                }
                if sources.count == 1 { Text("Open and track more scored sessions to train across animals.").font(.caption).foregroundStyle(.secondary) }
            }
            .font(.callout)
            if let r = result { report(r) }
            if let error { Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).font(.callout) }
            HStack {
                if busy { ProgressView().controlSize(.small); Text("Cross-validating…").font(.caption).foregroundStyle(.secondary) }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Train") { train() }.disabled(behavior.isEmpty || boutCount < 2 || busy)
                    .help("Needs at least 2 verified bouts")
                Button("Save Detector") {
                    if let json { _ = try? ModelLibrary.save(json); dismiss() }
                }
                .keyboardShortcut(.defaultAction).disabled(json == nil)
            }
        }
        .padding(20)
        .frame(width: 560)
        .onAppear { behavior = candidates.first { bouts($0, in: model).count >= 4 } ?? candidates.first ?? "" }
    }

    @ViewBuilder private func report(_ r: [String: Any]) -> some View {
        let ev = r["evaluation"] as? [String: Any] ?? [:], e = ev["frame"] as? [String: Any] ?? [:], bt = ev["bout"] as? [String: Any] ?? [:]
        let base = r["baseline"] as? [String: Any] ?? [:]
        let k = (e["kappa"] as? Double) ?? 0
        let (q, c) = quality(k)
        Label(q, systemImage: "checkmark.seal.fill").foregroundStyle(c)
        Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 4) {
            GridRow { Text("Frame agreement (κ)"); Text(String(format: "%.3f", k)).monospacedDigit() }
            GridRow { Text("PABAK"); Text(String(format: "%.3f", (e["pabak"] as? Double) ?? 0)).monospacedDigit() }
            GridRow { Text("Bout F1 (tIoU ≥ 0.5)"); Text(String(format: "%.3f", (bt["f1"] as? Double) ?? 0)).monospacedDigit() }
            GridRow { Text("Median onset error"); Text(String(format: "%.2f s", (bt["median_offset_s"] as? Double) ?? .nan)).monospacedDigit() }
            if let bk = (base["frame"] as? [String: Any])?["kappa"] as? Double {
                GridRow { Text("v1 detector, same folds"); Text(String(format: "κ %.3f", bk)).monospacedDigit().foregroundStyle(.secondary) }
            }
            GridRow { Text("Validation"); Text("\((ev["scheme"] as? String) ?? "") · \((ev["frames"] as? Int) ?? 0) frames") }
        }
        .font(.callout)
        let curve = (r["learning_curve"] as? [[String: Any]] ?? []).compactMap { p -> (Int, Double, Double)? in
            guard let n = p["bouts"] as? Int, let k = p["kappa"] as? Double, let f = p["bout_f1"] as? Double else { return nil }
            return (n, k, f)
        }
        if curve.count >= 2 {
            VStack(alignment: .leading, spacing: 4) {
                Text("Learning curve (held-out)").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                Chart {
                    ForEach(curve, id: \.0) { p in
                        LineMark(x: .value("Bouts", p.0), y: .value("Score", p.1), series: .value("Metric", "κ")).foregroundStyle(by: .value("Metric", "Frame κ"))
                        PointMark(x: .value("Bouts", p.0), y: .value("Score", p.1)).foregroundStyle(by: .value("Metric", "Frame κ"))
                        LineMark(x: .value("Bouts", p.0), y: .value("Score", p.2), series: .value("Metric", "F1")).foregroundStyle(by: .value("Metric", "Bout F1"))
                        PointMark(x: .value("Bouts", p.0), y: .value("Score", p.2)).foregroundStyle(by: .value("Metric", "Bout F1"))
                    }
                }
                .chartYScale(domain: -0.1...1)
                .chartXAxisLabel("Training bouts")
                .frame(height: 120)
                Text(verdict(curve)).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    /// Plain-language reading of the curve's last step.
    private func verdict(_ c: [(Int, Double, Double)]) -> String {
        let (a, b) = (c[c.count - 2], c[c.count - 1])
        return b.1 - a.1 > 0.05 ? "Still improving: scoring more bouts should help." : "Leveling off: more of the same bouts adds little; score other animals or conditions instead."
    }

    private func train() {
        busy = true
        error = nil
        let sessions = included.map { TrainingSession(name: $0.rec.title, session: $0.rec.session, tracker: $0.video.tracker!, camera: $0.video.camera, bouts: bouts(behavior, in: $0.video)) }
        let b = behavior
        Task {
            do {
                let out = try await Task.detached { try trainBehavior(sessions: sessions, behavior: b) }.value
                json = out
                result = (try? JSONSerialization.jsonObject(with: Data(out.utf8))) as? [String: Any]
            } catch {
                self.error = error.localizedDescription
            }
            busy = false
        }
    }
}

/// Proposals waiting for a decision: jump, watch, accept, reject or adjust.
struct ReviewQueue: View {
    @Bindable var model: VideoModel
    @State private var current: Int?

    var body: some View {
        // Least certain first: those are where a decision teaches the next model the most.
        let pending = model.annotations.indices.filter { model.annotations[$0].review == "proposed" }.sorted { (model.annotations[$0].confidence ?? 0) < (model.annotations[$1].confidence ?? 0) }
        if !pending.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Label("\(pending.count) proposals to review", systemImage: "tray.full").font(.caption.weight(.semibold))
                    Spacer()
                    Text("Unreviewed proposals never enter analysis").font(.caption2).foregroundStyle(.secondary)
                }
                ScrollView {
                    VStack(spacing: 2) {
                        ForEach(pending, id: \.self) { i in
                            let a = model.annotations[i]
                            HStack(spacing: 6) {
                                Button { current = i; model.seek(blockTime: a.startS); model.player.play() } label: { Image(systemName: "play.circle") }
                                    .buttonStyle(.borderless).help("Watch this bout")
                                Text(a.label.replacingOccurrences(of: "_", with: " ")).font(.caption.weight(.medium))
                                Text(String(format: "%.1f–%.1f s", a.startS, a.endS ?? a.startS)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                                Text(String(format: "%.0f %%", 100 * (a.confidence ?? 0))).font(.caption2.monospacedDigit()).foregroundStyle(.secondary)
                                Spacer()
                                if current == i, let t = model.blockTime {
                                    Button("Start ←") { edit(i) { $0.startS = t } }.help("Set the start to the current frame")
                                    Button("End ←") { edit(i) { $0.endS = t } }.help("Set the end to the current frame")
                                }
                                Button { decide(i, "accepted") } label: { Image(systemName: "checkmark") }.help("Accept")
                                Button { decide(i, "rejected") } label: { Image(systemName: "xmark") }.help("Reject")
                            }
                            .padding(.vertical, 2).padding(.horizontal, 4)
                            .background(current == i ? Color.accentColor.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 5))
                            .buttonStyle(.bordered).controlSize(.mini)
                        }
                    }
                }
                .frame(maxHeight: 120)
            }
        }
    }

    private func decide(_ i: Int, _ r: String) {
        guard model.annotations.indices.contains(i) else { return }
        model.player.pause()
        if model.annotations[i].review == "proposed" { model.annotations[i].review = r }
        model.dirty = true
        current = nil
    }

    private func edit(_ i: Int, _ f: (inout AnnotationJSON) -> Void) {
        guard model.annotations.indices.contains(i) else { return }
        f(&model.annotations[i])
        if let e = model.annotations[i].endS, e <= model.annotations[i].startS { model.error = "End must be after start"; return }
        model.annotations[i].review = "edited"
        model.dirty = true
    }
}
