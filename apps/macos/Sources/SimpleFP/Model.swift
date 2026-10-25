import AppKit
import Foundation
import Observation

/// One opened TDT block and its analysis state.
@MainActor @Observable
final class Recording: Identifiable {
    let id = UUID()
    let url: URL
    let session: Session
    let info: BlockInfo

    var preset: PresetInfo
    /// Resolved preset TOML actually used for the run (editable).
    var presetTOML: String
    var epocs: Set<String> = []
    var manualEvents: [EventSpec] = []
    var results: Results?
    var error: String?
    var isRunning = false
    /// Bumped whenever results change so plots refetch envelopes.
    var revision = 0
    var visible: ClosedRange<Double>
    var selectedEvent: String?
    /// Shared playhead on the block clock (video frame ↔ trace).
    var cursor: Double?
    var video: VideoModel?
    var status: String?
    var paramError: String?
    var rerun: Task<Void, Never>?

    init(url: URL, preset: PresetInfo) throws {
        self.url = url
        self.preset = preset
        session = try Session.open(path: url.path, presetToml: preset.toml)
        info = session.info()
        presetTOML = session.presetToml()
        visible = 0...max(info.durationS, 1)
        // Default: analyze around every epoc with a sensible number of events.
        // Camera strobes (Cam1…) and Tick are clocks, not behavior events.
        epocs = Set(info.epocs.filter { (2...2000).contains($0.count) && $0.name != "Tick" && !$0.name.hasPrefix("Cam") }.map(\.name))
    }

    var title: String { info.name }

    var eventSpecs: [EventSpec] {
        epocs.sorted().map { EventSpec.epoc(store: $0) } + manualEvents
    }

    func run(presetTOML newTOML: String? = nil) async {
        isRunning = true
        defer { isRunning = false }
        let session = session, specs = eventSpecs
        do {
            let r = try await Task.detached(priority: .userInitiated) {
                try session.analyze(presetToml: newTOML, events: specs)
            }.value
            if let newTOML { presetTOML = newTOML }
            results = r
            error = nil
            if selectedEvent == nil || !r.events.contains(where: { $0.label == selectedEvent }) {
                selectedEvent = r.events.first?.label
            }
            revision += 1
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Open the first camera video stored with the block, if any.
    func loadVideo() async {
        guard video == nil, let file = session.videos().first else { return }
        let v = VideoModel(rec: self, file: file, scorer: UserDefaults.standard.string(forKey: "labInitials").flatMap { $0.isEmpty ? nil : $0 } ?? NSUserName().prefix(2).uppercased())
        video = v
        await v.load()
        // Dev/automation flags: `--track` runs tracking on open, `--seek=<s>` moves the playhead.
        let args = CommandLine.arguments
        if args.contains("--track") { await v.analyzeVideo() }
        if let m = args.first(where: { $0.hasPrefix("--detect=") }), let tracker = v.tracker,
           let json = try? String(contentsOfFile: String(m.dropFirst(9)), encoding: .utf8),
           let out = try? proposeBehavior(modelJson: json, session: session, tracker: tracker, camera: v.camera),
           let props = try? JSONDecoder().decode([AnnotationJSON].self, from: Data(out.utf8)) {
            v.annotations += props
        }
        if let m = args.first(where: { $0.hasPrefix("--pose=") }) { await v.runPose(URL(fileURLWithPath: String(m.dropFirst(7)))) }
        if let s = args.first(where: { $0.hasPrefix("--seek=") }), let t = Double(s.dropFirst(7)) { v.seek(blockTime: t) }
    }

    func use(preset p: PresetInfo) async {
        preset = p
        await run(presetTOML: p.toml)
    }
}

@MainActor @Observable
final class Library {
    var recordings: [Recording] = []
    var cohorts: [CohortDoc] = []
    /// Shared id space: a recording or a cohort.
    var selection: UUID?
    var alert: String?
    var showNewCohort = false
    let presets: [PresetInfo] = SimpleFP.presets()

    var selected: Recording? { recordings.first { $0.id == selection } }
    var selectedCohort: CohortDoc? { cohorts.first { $0.id == selection } }

    func openCohort(_ url: URL) async {
        if let existing = cohorts.first(where: { $0.url == url }) {
            selection = existing.id
            await existing.run()
            return
        }
        let doc = CohortDoc(url: url)
        cohorts.append(doc)
        selection = doc.id
        await doc.run()
    }

    func openDemoCohort() async {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("SimpleFP Demo Cohort-\(Int(Date().timeIntervalSince1970))")
        do {
            let path = try writeDemoCohort(dir: dir.path)
            await openCohort(URL(fileURLWithPath: path))
        } catch {
            alert = error.localizedDescription
        }
    }

    func exportCohort(_ doc: CohortDoc) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Export"
        panel.directoryURL = doc.url.deletingLastPathComponent()
        guard panel.runModal() == .OK, let base = panel.url else { return }
        let out = base.appendingPathComponent("simplefp-cohort")
        do {
            try doc.export(to: out)
            NSWorkspace.shared.activateFileViewerSelecting([out])
        } catch {
            alert = error.localizedDescription
        }
    }

    /// Open a block, every block directly inside a tank folder, or a cohort manifest (.toml).
    func open(_ url: URL) async {
        if url.pathExtension.lowercased() == "toml" {
            await openCohort(url)
            return
        }
        let blocks: [URL]
        if isTdtBlock(dir: url.path) {
            blocks = [url]
        } else {
            let kids = (try? FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: [.isDirectoryKey])) ?? []
            blocks = kids.filter { isTdtBlock(dir: $0.path) }.sorted { $0.lastPathComponent < $1.lastPathComponent }
        }
        guard !blocks.isEmpty else {
            alert = "“\(url.lastPathComponent)” is not a TDT block or tank. Choose a folder that contains a .tsq file."
            return
        }
        for b in blocks {
            if let existing = recordings.first(where: { $0.url == b }) {
                selection = existing.id
                continue
            }
            do {
                let rec = try Recording(url: b, preset: presets[0])
                recordings.append(rec)
                selection = rec.id
                await rec.run()
            } catch {
                alert = "Couldn’t open \(b.lastPathComponent): \(error.localizedDescription)"
            }
        }
    }

    func openDemo() async {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("SimpleFP Demo-\(Int(Date().timeIntervalSince1970))")
        do {
            _ = try writeDemo(dir: dir.path, seed: 1)
            await open(dir)
            // Render a matching arena video so scoring can be tried end to end.
            guard let rec = recordings.first(where: { $0.url == dir }) else { return }
            rec.status = "Rendering demo video…"
            let clock = try rec.session.frameClock(camera: "Cam1")
            let times = clock.times.withUnsafeBytes { Array($0.bindMemory(to: Double.self)) }
            let events = rec.results?.events.first?.eventTimes ?? []
            let url = dir.appendingPathComponent("\(dir.lastPathComponent)_Cam1.mp4")
            // Two trailing frames past the last strobe, as real TDT recordings have (TN0987).
            try await DemoVideo.write(to: url, frames: times.count + 2, fps: 15, frameTimes: times, events: events)
            rec.status = nil
            await rec.loadVideo()
        } catch {
            alert = error.localizedDescription
        }
    }

    func close(_ doc: CohortDoc) {
        cohorts.removeAll { $0.id == doc.id }
        if selection == doc.id { selection = recordings.first?.id ?? cohorts.first?.id }
    }

    func close(_ rec: Recording) {
        recordings.removeAll { $0.id == rec.id }
        if selection == rec.id { selection = recordings.first?.id }
    }

    func presentOpenPanel() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.allowedContentTypes = [.folder, .toml]
        panel.allowsMultipleSelection = true
        panel.message = "Choose a TDT block, a tank folder of blocks, or a cohort manifest (.toml)"
        panel.prompt = "Open"
        guard panel.runModal() == .OK else { return }
        let urls = panel.urls
        Task { for u in urls { await open(u) } }
    }

    func presentExportPanel() {
        guard let rec = selected, rec.results != nil else { return }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Export"
        panel.message = "Choose where to write CSVs and run.json for \(rec.title)"
        panel.directoryURL = rec.url
        guard panel.runModal() == .OK, let base = panel.url else { return }
        let out = base.appendingPathComponent("simplefp-\(rec.title)")
        do {
            let n = try rec.session.export(dir: out.path)
            NSWorkspace.shared.activateFileViewerSelecting([out])
            _ = n
        } catch {
            alert = error.localizedDescription
        }
    }

    func importEvents(into rec: Recording) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.commaSeparatedText, .plainText]
        panel.message = "CSV with header event,time_s (seconds on the TDT block clock)"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            rec.manualEvents = try readEventsCsv(path: url.path)
            Task { await rec.run() }
        } catch {
            alert = error.localizedDescription
        }
    }
}

extension EventSpec {
    var label: String {
        switch self {
        case .epoc(let s): s
        case .epocOffset(let s): s + "_offset"
        case .manual(let l, _): l
        }
    }
}

/// Decode little-endian f32 payloads from the engine.
extension Data {
    func floats() -> [Float] {
        withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
    }
}
