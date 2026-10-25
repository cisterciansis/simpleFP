import AppKit
import SwiftUI

extension Recording {
    /// Current preset as a JSON tree (values for the Easy controls).
    var params: [String: Any] {
        (try? JSONSerialization.jsonObject(with: Data(presetJson(tomlSrc: presetTOML).utf8))) as? [String: Any] ?? [:]
    }

    func param(_ path: String) -> Any? {
        path.split(separator: ".").reduce(params as Any?) { ($0 as? [String: Any])?[String($1)] }
    }

    func number(_ path: String) -> Double { (param(path) as? NSNumber)?.doubleValue ?? 0 }
    func string(_ path: String) -> String { param(path) as? String ?? "" }

    /// Change one parameter through the engine's validator, then re-run (debounced).
    func setParam(_ key: String, _ value: String) {
        do {
            presetTOML = try presetSet(tomlSrc: presetTOML, key: key, value: value)
            paramError = nil
            rerun?.cancel()
            let t = presetTOML
            rerun = Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(250))
                guard !Task.isCancelled else { return }
                await self?.run(presetTOML: t)
            }
        } catch {
            paramError = error.localizedDescription
        }
    }
}

/// Plain-language analysis controls. Every change is validated by the engine and
/// re-runs the analysis live; "Code" shows the same settings as TOML.
struct EasyParameters: View {
    let rec: Recording
    @State private var showCode = false

    private func slider(_ title: String, _ path: String, _ range: ClosedRange<Double>, step: Double, unit: String, help: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            LabeledContent(title) { Text(String(format: step < 1 ? "%.1f %@" : "%.0f %@", rec.number(path), unit)).monospacedDigit() }
            Slider(value: Binding(get: { rec.number(path) }, set: { rec.setParam(path, String(format: "%.3f", ($0 / step).rounded() * step)) }), in: range)
            Text(help).font(.caption).foregroundStyle(.secondary)
        }
    }

    private func seconds(_ title: String, _ path: String, _ range: ClosedRange<Double>) -> some View {
        Stepper(value: Binding(get: { rec.number(path) }, set: { rec.setParam(path, String(format: "%.1f", $0)) }), in: range, step: 1) {
            LabeledContent(title) { Text(String(format: "%.0f s", rec.number(path))).monospacedDigit() }
        }
    }

    private func window(_ title: String, _ path: String, _ range: ClosedRange<Double>, help: String) -> some View {
        let w = (rec.param(path) as? [NSNumber])?.map(\.doubleValue) ?? [0, 1]
        let set = { (a: Double, b: Double) in rec.setParam(path, String(format: "[%.1f, %.1f]", a, b)) }
        return VStack(alignment: .leading, spacing: 4) {
            LabeledContent(title) { Text(String(format: "%.0f → %.0f s", w[0], w[1])).monospacedDigit() }
            HStack {
                Stepper("from", value: Binding(get: { w[0] }, set: { set($0, w[1]) }), in: range, step: 1).labelsHidden()
                Text("from").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Text("to").font(.caption).foregroundStyle(.secondary)
                Stepper("to", value: Binding(get: { w[1] }, set: { set(w[0], $0) }), in: range, step: 1).labelsHidden()
            }
            Text(help).font(.caption).foregroundStyle(.secondary)
        }
    }

    var body: some View {
        let bleach = rec.string("bleach.method"), motion = rec.string("motion.method")
        Group {
            Section("Signal") {
                slider("Skip start", "preprocess.trim_start_s", 0...60, step: 1, unit: "s", help: "Ignore the first seconds while the LEDs settle.")
                slider("Smoothing", "preprocess.lowpass_hz", 1...9.5, step: 0.5, unit: "Hz", help: "Removes fast noise. GRAB-ACh3.0 rises in ~0.1 s, so 4 Hz keeps every real transient.")
            }
            Section("Bleaching") {
                Picker("Correction", selection: Binding(get: { bleach }, set: { m in
                    if m == "window" {
                        rec.setParam("bleach.method", "\"window\"")
                        rec.setParam("bleach.start_s", "30.0"); rec.setParam("bleach.end_s", "120.0")
                    } else if m == "double_exp" {
                        rec.setParam("bleach.method", "\"double_exp\""); rec.setParam("bleach.robust", "false")
                    } else {
                        rec.setParam("bleach.method", "\"\(m)\"")
                    }
                })) {
                    Text("Automatic curve").tag("double_exp")
                    Text("Straight line").tag("linear")
                    Text("Fixed reference period").tag("window")
                    Text("Off").tag("none")
                }
                Text(["double_exp": "Fits the usual fading of the sensor and removes it.", "linear": "A gentle straight-line correction for short or irregular sessions.",
                      "window": "Measures everything relative to a quiet reference period. Use this when the experiment changes the animal's state (e.g. anesthesia → awake), so the change isn't erased.",
                      "none": "No correction: only for very short, stable recordings."][bleach] ?? "")
                    .font(.caption).foregroundStyle(.secondary)
                if bleach == "window" {
                    seconds("Reference from", "bleach.start_s", 0...3600)
                    seconds("Reference to", "bleach.end_s", 0...3600)
                }
            }
            Section("Motion (\(rec.info.control ?? "control"))") {
                Picker("Correction", selection: Binding(get: { motion }, set: { m in
                    rec.setParam("motion.method", "\"\(m)\"")
                    if m == "irls" || m == "fit_raw" { rec.setParam("motion.tuning", "1.4") }
                })) {
                    Text("Robust (recommended)").tag("irls")
                    Text("Standard").tag("ols")
                    Text("Combined fit").tag("fit_raw")
                    Text("Off").tag("none")
                }
                .disabled(rec.info.control == nil)
                if motion == "irls" || motion == "fit_raw" {
                    slider("Strictness", "motion.tuning", 1.4...4.7, step: 0.1, unit: "", help: "How strongly big real responses are ignored when matching the control (1.4 = strict, as validated; 4.7 = classic).")
                }
            }
            Section("Events") {
                seconds("Before event", "events.pre_s", 1...600)
                seconds("After event", "events.post_s", 1...600)
                Picker("Per-trial baseline", selection: Binding(get: { rec.string("events.baseline") }, set: { rec.setParam("events.baseline", "\"\($0)\"") })) {
                    Text("Subtract").tag("subtract"); Text("z-score").tag("zscore"); Text("Robust z").tag("robust_zscore"); Text("None").tag("none")
                }
                window("Baseline", "events.baseline_window", -rec.number("events.pre_s")...0, help: "Before the event; at least 2 s.")
                window("Response", "events.metric_window", -rec.number("events.pre_s")...rec.number("events.post_s"), help: "Where mean, area, peak and trough are measured. Fix it before looking at results.")
            }
            if let e = rec.paramError {
                Label(e, systemImage: "exclamationmark.octagon.fill").foregroundStyle(.red).font(.callout)
            }
            HStack {
                Button("Code…") { showCode = true }.help("The same settings as TOML: edit, save or share them")
                Button("Copy") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(rec.presetTOML, forType: .string)
                }
                .help("Copy the fully resolved preset (paste into a .toml file or a cohort)")
                Button("Save As…") {
                    let panel = NSSavePanel()
                    panel.allowedContentTypes = [.toml]
                    panel.nameFieldStringValue = "\(rec.preset.id)-custom.toml"
                    if panel.runModal() == .OK, let url = panel.url { try? rec.presetTOML.write(to: url, atomically: true, encoding: .utf8) }
                }
                Spacer()
                Button("Reset") { Task { await rec.use(preset: rec.preset) } }.help("Back to the \(rec.preset.name) defaults")
            }
        }
        .sheet(isPresented: $showCode) { PresetEditor(rec: rec) }
    }
}
