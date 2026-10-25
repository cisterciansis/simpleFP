import SwiftUI

struct ContentView: View {
    @Environment(Library.self) private var library
    @Environment(LabModel.self) private var lab
    @State private var showInspector = false
    @State private var columns: NavigationSplitViewVisibility = .automatic
    @State private var dropTargeted = false

    var body: some View {
        @Bindable var library = library
        NavigationSplitView(columnVisibility: $columns) {
            Sidebar()
                .navigationSplitViewColumnWidth(min: 200, ideal: 240, max: 320)
        } detail: {
            Group {
                if let rec = library.selected {
                    RecordingView(rec: rec)
                } else if let doc = library.selectedCohort {
                    CohortView(doc: doc)
                } else if library.selection == LabModel.animalsID {
                    AnimalsView()
                } else if library.selection == LabModel.cagesID {
                    CagesView()
                } else if library.selection == LabModel.tasksID {
                    TasksView()
                } else {
                    EmptyState()
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        showInspector.toggle()
                        // Make room on narrow screens the way Xcode does: hide the sidebar while inspecting.
                        if showInspector, (NSApp.keyWindow?.frame.width ?? 0) < 1280 { withAnimation { columns = .detailOnly } }
                    } label: { Label("Inspector", systemImage: "sidebar.trailing") }
                        .disabled(library.selected == nil)
                        .help("Show or hide the inspector")
                }
            }
        }
        .inspector(isPresented: $showInspector) {
            Group {
                if let rec = library.selected { InspectorView(rec: rec) } else if let doc = library.selectedCohort { CohortInspector(doc: doc) } else { Color.clear }
            }
            .inspectorColumnWidth(min: 240, ideal: 300, max: 380)
        }
        .dropDestination(for: URL.self) { urls, _ in
            Task { for u in urls { await library.open(u) } }
            return !urls.isEmpty
        } isTargeted: { dropTargeted = $0 }
        .overlay {
            if dropTargeted {
                RoundedRectangle(cornerRadius: 14).strokeBorder(Color.accentColor, style: StrokeStyle(lineWidth: 3, dash: [8, 6]))
                    .padding(8).allowsHitTesting(false)
            }
        }
        .task {
            lab.open()
            // `SimpleFP <block>…` or `SimpleFP --demo` (e.g. via `open -a SimpleFP --args …`).
            for arg in CommandLine.arguments.dropFirst() {
                if arg == "--seed-demo-lab", lab.animals.isEmpty { lab.seedDemo() }
                if arg == "--inspector" { columns = .detailOnly; showInspector = true }
                if arg == "--lab" { library.selection = LabModel.cagesID } else if arg == "--animals" { library.selection = LabModel.animalsID } else if arg == "--tasks" { library.selection = LabModel.tasksID } else if arg == "--demo" { await library.openDemo() } else if arg == "--demo-cohort" { await library.openDemoCohort() } else if FileManager.default.fileExists(atPath: arg) { await library.open(URL(fileURLWithPath: arg)) }
            }
        }
        .sheet(isPresented: $library.showNewCohort) { NewCohortSheet() }
        .alert("SimpleFP", isPresented: Binding(get: { library.alert != nil }, set: { if !$0 { library.alert = nil } })) {
            Button("OK") { library.alert = nil }
        } message: {
            Text(library.alert ?? "")
        }
    }
}

private struct Sidebar: View {
    @Environment(Library.self) private var library
    @Environment(LabModel.self) private var lab

    var body: some View {
        @Bindable var library = library
        List(selection: $library.selection) {
            Section("Lab") {
                Label("Animals", systemImage: "pawprint").tag(LabModel.animalsID)
                Label("Cages", systemImage: "square.grid.2x2").tag(LabModel.cagesID)
                Label("Tasks", systemImage: "checklist").tag(LabModel.tasksID)
                    .badge(lab.tasks.filter { $0.overdue || $0.dueOn <= String(Date().formatted(.iso8601.year().month().day())) }.count)
            }
            if !library.cohorts.isEmpty {
                Section("Cohorts") {
                    ForEach(library.cohorts) { doc in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(doc.title).font(.body.weight(.medium)).lineLimit(1)
                            HStack(spacing: 6) {
                                if doc.isRunning { ProgressView().controlSize(.mini) }
                                if let r = doc.results {
                                    Text("\(Set(r.animals.map(\.animal)).count) animals · \(r.sessions.count) sessions")
                                }
                                if doc.error != nil { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.yellow) }
                            }
                            .font(.caption).foregroundStyle(.secondary)
                        }
                        .padding(.vertical, 2)
                        .tag(doc.id)
                        .contextMenu {
                            Button("Show Manifest in Finder") { NSWorkspace.shared.activateFileViewerSelecting([doc.url]) }
                            Divider()
                            Button("Close", role: .destructive) { library.close(doc) }
                        }
                    }
                }
            }
            Section("Recordings") {
                ForEach(library.recordings) { rec in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(rec.title).font(.body.weight(.medium)).lineLimit(1).truncationMode(.middle)
                        HStack(spacing: 6) {
                            if rec.isRunning { ProgressView().controlSize(.mini) }
                            Text(formatClock(rec.info.durationS))
                            Text("·")
                            Text(rec.preset.sensor)
                            if rec.error != nil { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.yellow) }
                        }
                        .font(.caption).foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 2)
                    .tag(rec.id)
                    .contextMenu {
                        Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([rec.url]) }
                        Divider()
                        Button("Close", role: .destructive) { library.close(rec) }
                    }
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            HStack {
                Menu {
                    Button("Open Block, Tank or Cohort…") { library.presentOpenPanel() }
                    Button("New Cohort from Open Recordings…") { library.showNewCohort = true }
                        .disabled(library.recordings.isEmpty)
                    Divider()
                    Button("Demo Recording") { Task { await library.openDemo() } }
                    Button("Demo Cohort") { Task { await library.openDemoCohort() } }
                } label: { Label("Open", systemImage: "plus") }
                    .menuStyle(.borderlessButton).fixedSize()
                Spacer()
            }
            .padding(10)
        }
        .overlay {
            EmptyView()
        }
    }
}

private struct EmptyState: View {
    @Environment(Library.self) private var library

    var body: some View {
        ContentUnavailableView {
            Label("Drop a TDT block here", systemImage: "waveform.path.ecg")
        } description: {
            Text("Open a Synapse block or a tank of blocks. Streams, epocs and presets are detected automatically.")
        } actions: {
            HStack {
                Button("Open TDT Block…") { library.presentOpenPanel() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut("o")
                Button("Try Demo Recording") { Task { await library.openDemo() } }
            }
            .controlSize(.large)
        }
    }
}

private struct RecordingView: View {
    @Environment(Library.self) private var library
    let rec: Recording
    @State private var mode: TraceMode = .dff
    @State private var showVideo = true

    var body: some View {
        VSplitView {
            VStack(spacing: 6) {
                header
                HStack(alignment: .top, spacing: 10) {
                    if let v = rec.video, showVideo {
                        VideoPane(model: v).frame(minWidth: 260, idealWidth: 380, maxWidth: 520)
                    }
                    VStack(spacing: 6) {
                        TracePlot(rec: rec, mode: mode)
                            .frame(minHeight: 180)
                        OverviewStrip(rec: rec)
                    }
                }
            }
            .padding(.horizontal, 12).padding(.top, 8).padding(.bottom, 10)
            .frame(maxWidth: .infinity, minHeight: 280, maxHeight: .infinity)

            EventPanel(rec: rec)
                .frame(maxWidth: .infinity, minHeight: 220, maxHeight: .infinity)
        }
        .task(id: rec.id) { await rec.loadVideo() }
        .onDisappear { rec.video?.player.pause() }
        .navigationTitle(rec.title)
        .navigationSubtitle(subtitle)
        .toolbar {
            ToolbarItem {
                Button { showVideo.toggle() } label: { Label("Video", systemImage: showVideo ? "video.fill" : "video") }
                    .disabled(rec.video == nil)
                    .help(rec.video == nil ? "No camera video in this block (…Cam1.mp4)" : "Show or hide the synchronized video and scoring")
            }
            ToolbarItem {
                EventsMenu(rec: rec)
            }
            ToolbarItem {
                Button { library.presentExportPanel() } label: { Label("Export", systemImage: "square.and.arrow.up") }
                    .disabled(rec.results == nil)
                    .help("Export CSVs and a provenance manifest (⌘E)")
            }
        }
    }

    private var subtitle: String {
        let d = Date(timeIntervalSince1970: rec.info.startUnix)
        return "\(d.formatted(date: .abbreviated, time: .shortened)) · \(rec.preset.sensor)"
    }

    @ViewBuilder private var header: some View {
        if let e = rec.error {
            Label(e, systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.yellow).font(.callout)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(8).background(.yellow.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
        } else if let qc = rec.results?.qc {
            HStack(spacing: 8) {
                // Drop chips as the window narrows rather than overflowing it.
                ViewThatFits(in: .horizontal) {
                    chips(qc, full: true)
                    chips(qc, full: false)
                    Color.clear.frame(width: 0, height: 0)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if rec.isRunning { ProgressView().controlSize(.small) }
                if let st = rec.status { ProgressView().controlSize(.small); Text(st).font(.caption).foregroundStyle(.secondary) }
                modePicker
            }
        }
    }

    private func chips(_ qc: Qc, full: Bool) -> some View {
        HStack(spacing: 8) {
            Chip("Bleaching", String(format: "%.0f %%", qc.bleachSignalPct), ok: qc.bleachSignalPct < 50 && !qc.bleachFallback)
            if let r = qc.motionR { Chip("Control fit", String(format: "r %.2f", r), ok: (qc.motionSlope ?? 1) > 0) }
            if full {
                Chip("Noise", String(format: "%.2f %%", qc.noiseDffPct), ok: true)
                Chip("Rate", String(format: "%.1f Hz", qc.fs), ok: true)
            }
            if !qc.warnings.isEmpty {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.yellow)
                    .help(qc.warnings.joined(separator: "\n"))
            }
        }
        .fixedSize()
    }

    private var modePicker: some View {
        Picker("Trace", selection: $mode) {
            ForEach(TraceMode.allCases) { Text($0.rawValue).tag($0) }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .help("ΔF/F, session z-score, or raw signal with fitted control and bleaching baseline")
    }
}

private struct Chip: View {
    let title: String, value: String, ok: Bool
    init(_ title: String, _ value: String, ok: Bool) { self.title = title; self.value = value; self.ok = ok }

    var body: some View {
        HStack(spacing: 5) {
            Circle().fill(ok ? Color.green : Color.orange).frame(width: 6, height: 6)
            Text(title).foregroundStyle(.secondary)
            Text(value).monospacedDigit().fontWeight(.medium)
        }
        .font(.caption)
        .padding(.horizontal, 9).padding(.vertical, 4)
        .background(.quaternary.opacity(0.6), in: Capsule())
    }
}

private struct EventsMenu: View {
    @Environment(Library.self) private var library
    let rec: Recording

    var body: some View {
        Menu {
            Section("TDT epocs") {
                ForEach(rec.info.epocs, id: \.name) { e in
                    Toggle("\(e.name)  (\(e.count))", isOn: Binding(
                        get: { rec.epocs.contains(e.name) },
                        set: { on in
                            if on { rec.epocs.insert(e.name) } else { rec.epocs.remove(e.name) }
                            Task { await rec.run() }
                        }))
                }
                if rec.info.epocs.isEmpty { Text("No epocs in this block") }
            }
            Section("Manual") {
                Button("Import Events CSV…") { library.importEvents(into: rec) }
                if !rec.manualEvents.isEmpty {
                    Button("Clear \(rec.manualEvents.count) imported event set(s)") {
                        rec.manualEvents = []
                        Task { await rec.run() }
                    }
                }
            }
        } label: {
            Label("Events", systemImage: "flag")
        }
        .help("Choose which events to align to")
    }
}

private struct EventPanel: View {
    let rec: Recording

    var body: some View {
        @Bindable var rec = rec
        let events = rec.results?.events ?? []
        if let ev = events.first(where: { $0.label == rec.selectedEvent }) ?? events.first {
            let idx = events.firstIndex { $0.label == ev.label } ?? 0
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    if events.count > 1 {
                        Picker("Event", selection: $rec.selectedEvent) {
                            ForEach(events, id: \.label) { Text($0.label).tag(Optional($0.label)) }
                        }
                        .pickerStyle(.segmented).labelsHidden().fixedSize()
                    } else {
                        Text(ev.label).font(.headline)
                    }
                    Text("\(ev.eventTimes.count) trials" + (ev.dropped.isEmpty ? "" : " · \(ev.dropped.count) outside analyzed range"))
                        .font(.callout).foregroundStyle(.secondary)
                    Spacer()
                    if !ev.controlMean.isEmpty {
                        HStack(spacing: 4) {
                            RoundedRectangle(cornerRadius: 1).fill(Palette.control).frame(width: 14, height: 2)
                            Text(rec.info.control ?? "control").font(.caption).foregroundStyle(.secondary)
                        }
                        .help("Control channel, same epochs and baseline (% of its own baseline)")
                    }
                    if ev.controlResponds {
                        Label("Control responds", systemImage: "exclamationmark.triangle.fill")
                            .font(.caption).foregroundStyle(.orange)
                            .help("The control channel's CI excludes 0 in the response window. Part of the signal response may be shared with the control; confirm with a ligand-insensitive (mutant) sensor.")
                    }
                    if !ev.significant.isEmpty {
                        HStack(spacing: 4) {
                            RoundedRectangle(cornerRadius: 1).fill(Palette.significant).frame(width: 14, height: 3)
                            Text("95% CI excludes 0").font(.caption).foregroundStyle(.secondary)
                        }
                        .help("Bootstrap CI across trials (expanded by √(n/(n−1))), runs ≥ 1/low-pass seconds. Trials of one recording: descriptive, not group inference.")
                    }
                }
                // Reflow for narrow windows: drop the table, then the heatmap.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 12) {
                        PethPlot(ev: ev, color: Palette.events(idx)).frame(minWidth: 200, maxWidth: .infinity)
                        EventHeatmap(ev: ev).frame(minWidth: 200, maxWidth: .infinity)
                        MetricsTable(ev: ev).frame(minWidth: 300, idealWidth: 360, maxWidth: 420)
                    }
                    HStack(spacing: 12) {
                        PethPlot(ev: ev, color: Palette.events(idx)).frame(minWidth: 180, maxWidth: .infinity)
                        EventHeatmap(ev: ev).frame(minWidth: 180, maxWidth: .infinity)
                    }
                    PethPlot(ev: ev, color: Palette.events(idx)).frame(minWidth: 120, maxWidth: .infinity)
                }
            }
            .padding(12)
        } else {
            ContentUnavailableView("No events selected", systemImage: "flag.slash", description: Text("Pick TDT epocs or import an events CSV from the Events menu."))
        }
    }
}

private struct MetricRow: Identifiable {
    let id: Int
    let m: TrialMetrics
}

private struct MetricsTable: View {
    let ev: EventSummary

    var body: some View {
        let rows = ev.metrics.enumerated().map { MetricRow(id: $0.offset + 1, m: $0.element) }
        Table(rows) {
            TableColumn("#") { Text("\($0.id)").monospacedDigit() }.width(24)
            TableColumn("t (s)") { Text(String(format: "%.1f", $0.m.eventTime)).monospacedDigit() }.width(min: 40, ideal: 56)
            TableColumn("Mean") { Text(String(format: "%.2f", $0.m.mean)).monospacedDigit() }.width(min: 34, ideal: 52)
            TableColumn("AUC") { Text(String(format: "%.2f", $0.m.auc)).monospacedDigit() }.width(min: 34, ideal: 52)
            TableColumn("Peak") { Text(String(format: "%.2f", $0.m.peak)).monospacedDigit() }.width(min: 34, ideal: 52)
            TableColumn("Trough") { Text(String(format: "%.2f", $0.m.trough)).monospacedDigit() }.width(min: 34, ideal: 52)
        }
        .tableStyle(.bordered(alternatesRowBackgrounds: true))
        .font(.callout)
    }
}

private struct InspectorView: View {
    @Environment(Library.self) private var library
    let rec: Recording

    var body: some View {
        Form {
            Section("Preset") {
                Picker("Preset", selection: Binding(get: { rec.preset.id }, set: { id in
                    if let p = library.presets.first(where: { $0.id == id }) { Task { await rec.use(preset: p) } }
                })) {
                    ForEach(library.presets, id: \.id) { Text($0.name).tag($0.id) }
                }
                Text(rec.preset.description).font(.callout).foregroundStyle(.secondary)
            }
            EasyParameters(rec: rec)
            if let qc = rec.results?.qc {
                Section("Quality") {
                    LabeledContent("Analyzed", value: "\(formatClock(qc.analyzedFromS)) – \(formatClock(qc.analyzedToS))")
                    LabeledContent("Sample rate", value: String(format: "%.3f Hz (÷%d)", qc.fs, qc.decimation))
                    LabeledContent("Bleaching 465", value: String(format: "%.1f %%", qc.bleachSignalPct))
                    if let b = qc.bleachControlPct { LabeledContent("Bleaching 405", value: String(format: "%.1f %%", b)) }
                    if let r = qc.motionR, let m = qc.motionSlope {
                        LabeledContent("Control fit", value: String(format: "r = %.3f, β = %.3f", r, m))
                    }
                    if let z = qc.motionZeroWeightFrac {
                        LabeledContent("Rejected by IRLS", value: String(format: "%.1f %%", z * 100))
                    }
                    LabeledContent("Residual drift", value: String(format: "%+.2f %%/10 min", qc.residualDriftPctPer10min))
                    LabeledContent("Noise", value: String(format: "%.3f %% ΔF/F", qc.noiseDffPct))
                    LabeledContent("Clipping", value: qc.saturationFrac > 0 ? String(format: "%.2f %%", qc.saturationFrac * 100) : "none")
                    ForEach(qc.warnings, id: \.self) { Label($0, systemImage: "exclamationmark.triangle").foregroundStyle(.orange) }
                }
            }
            Section("Block") {
                LabeledContent("Signal", value: rec.info.signal)
                LabeledContent("Control", value: rec.info.control ?? "–")
                ForEach(rec.info.streams, id: \.name) { s in
                    LabeledContent(s.name, value: String(format: "%.2f Hz · %d ch", s.fs, s.channels))
                }
                LabeledContent("Engine", value: engineVersion())
            }
        }
        .formStyle(.grouped)
    }
}

struct PresetEditor: View {
    @Environment(\.dismiss) private var dismiss
    let rec: Recording
    @State private var text = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Preset parameters").font(.title3.weight(.semibold))
            Text("Fully resolved TOML. Unknown keys and out-of-range values are rejected.").font(.callout).foregroundStyle(.secondary)
            TextEditor(text: $text)
                .font(.system(.body, design: .monospaced))
                .scrollContentBackground(.hidden)
                .padding(6)
                .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 8))
            if let error { Label(error, systemImage: "xmark.octagon.fill").foregroundStyle(.red).font(.callout) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Apply") {
                    Task {
                        await rec.run(presetTOML: text)
                        if let e = rec.error { error = e } else { dismiss() }
                    }
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 560, height: 620)
        .onAppear { text = rec.presetTOML }
    }
}
