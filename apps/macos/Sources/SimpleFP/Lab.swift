import AppKit
import CoreImage.CIFilterBuiltins
import SwiftUI
import UniformTypeIdentifiers

// MARK: - Rows (decoded from the engine's JSON; keys are column names)

struct AnimalRow: Codable, Identifiable, Hashable {
    let id: String
    let label: String
    let sex: String
    let lineId: String?
    let dob: String?
    let arrivedOn: String?
    let source: String
    let protocolId: String
    let earTag: String?
    let deathDate: String?
}

struct CageRow: Codable, Identifiable, Hashable {
    let id: String
    let label: String
    let kind: String
    let room: String?
    let rack: String?
    let position: Int?
    let capacity: Int
    let protocolId: String
    let closedOn: String?
}

struct LineRow: Codable, Identifiable, Hashable { let id: String; let name: String; let background: String }
struct ProtocolRow: Codable, Identifiable, Hashable { let id: String; let number: String; let title: String; let expiresOn: String }
struct AssignmentRow: Codable, Hashable { let animalId: String; let cageId: String; let endAt: Int? }
struct CohortRow: Codable, Identifiable, Hashable { let id: String; let name: String; let question: String?; let frozenAt: Int?; let membersHash: String? }
struct SessionLabRow: Codable, Identifiable, Hashable { let id: String; let animalId: String; let blockName: String; let paradigm: String; let condition: String? }
struct TaskRow: Codable, Hashable { let dueOn: String; let kind: String; let title: String; let entity: String; let entityId: String; let overdue: Bool }
struct TimelineRow: Codable, Hashable { let when: String; let kind: String; let title: String; let detail: String; let entity: String; let entityId: String }
struct VirusRow: Codable, Identifiable, Hashable { let id: String; let construct: String; let titerVgPerMl: Double }

private let decoder: JSONDecoder = {
    let d = JSONDecoder()
    d.keyDecodingStrategy = .convertFromSnakeCase
    return d
}()

private func today() -> String {
    let f = DateFormatter()
    f.calendar = Calendar(identifier: .gregorian)
    f.locale = Locale(identifier: "en_US_POSIX")
    f.dateFormat = "yyyy-MM-dd"
    return f.string(from: Date())
}

private func json(_ v: [String: Any?]) -> String {
    let clean = v.mapValues { $0 ?? NSNull() }
    return (try? String(data: JSONSerialization.data(withJSONObject: clean), encoding: .utf8)) ?? "{}"
}

// MARK: - Model

@MainActor @Observable
final class LabModel {
    static let animalsID = UUID(), cagesID = UUID(), tasksID = UUID()

    private(set) var store: LabStore?
    var animals: [AnimalRow] = []
    var cages: [CageRow] = []
    var lines: [LineRow] = []
    var protocols: [ProtocolRow] = []
    var cohorts: [CohortRow] = []
    var sessions: [SessionLabRow] = []
    var viruses: [VirusRow] = []
    var tasks: [TaskRow] = []
    var location: [String: String] = [:] // animal → cage
    var error: String?
    var toast: String?

    var initials: String {
        get { UserDefaults.standard.string(forKey: "labInitials").flatMap { $0.isEmpty ? nil : $0 } ?? NSUserName().prefix(2).uppercased() }
        set { UserDefaults.standard.set(newValue, forKey: "labInitials") }
    }

    var url: URL {
        // SIMPLEFP_LAB points at an alternate store (tests, demos) so the real lab file is never touched.
        if let p = ProcessInfo.processInfo.environment["SIMPLEFP_LAB"] { return URL(fileURLWithPath: p) }
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("SimpleFP", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("lab.sqlite")
    }

    func open() {
        guard store == nil else { return }
        do {
            store = try LabStore.open(path: url.path, actor: initials)
            reload()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func rows<T: Decodable>(_ table: String, _ type: T.Type) -> [T] {
        guard let store, let s = try? store.list(table: table, filterColumn: nil, filterValue: nil) else { return [] }
        do { return try decoder.decode([T].self, from: Data(s.utf8)) } catch {
            self.error = "\(table): \(error)"
            return []
        }
    }

    func reload() {
        animals = rows("animal", AnimalRow.self).sorted { $0.label.localizedStandardCompare($1.label) == .orderedAscending }
        cages = rows("cage", CageRow.self).filter { $0.closedOn == nil }.sorted { ($0.rack ?? "", $0.position ?? 0, $0.label) < ($1.rack ?? "", $1.position ?? 0, $1.label) }
        lines = rows("line", LineRow.self)
        protocols = rows("protocol", ProtocolRow.self)
        cohorts = rows("cohort", CohortRow.self)
        sessions = rows("session", SessionLabRow.self)
        viruses = rows("virus_lot", VirusRow.self)
        location = Dictionary(rows("cage_assignment", AssignmentRow.self).filter { $0.endAt == nil }.map { ($0.animalId, $0.cageId) }, uniquingKeysWith: { a, _ in a })
        if let store, let t = try? store.tasks(today: today(), weanAgeDays: 21) {
            tasks = (try? decoder.decode([TaskRow].self, from: Data(t.utf8))) ?? []
        }
    }

    /// Run a store action; surface errors, reload, and show warnings as a toast.
    func act(_ f: (LabStore) throws -> [String]) {
        guard let store else { return }
        do {
            let warnings = try f(store)
            toast = warnings.isEmpty ? nil : warnings.joined(separator: " · ")
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        reload()
    }

    func timeline(_ animal: String) -> [TimelineRow] {
        guard let store, let s = try? store.timeline(animalId: animal) else { return [] }
        return (try? decoder.decode([TimelineRow].self, from: Data(s.utf8))) ?? []
    }

    func occupants(_ cage: String) -> [AnimalRow] { animals.filter { location[$0.id] == cage && $0.deathDate == nil } }
    func line(_ id: String?) -> String { lines.first { $0.id == id }?.name ?? "—" }
    func cageLabel(_ animal: String) -> String { cages.first { $0.id == location[animal] }?.label ?? "—" }

    func seedDemo() {
        let dir = url.deletingLastPathComponent().appendingPathComponent("Demo data", isDirectory: true)
        act { try $0.seedDemo(dir: dir.path, today: today()); return ["Demo colony loaded"] }
    }

    /// Freeze (if needed), write the analysis manifest and hand it to the library.
    func analyze(cohort: CohortRow, library: Library) {
        guard let store else { return }
        do {
            if cohort.frozenAt == nil { _ = try store.freezeCohort(cohortId: cohort.id) }
            let dataRoot = url.deletingLastPathComponent().appendingPathComponent("Demo data").path
            let out = url.deletingLastPathComponent().appendingPathComponent("cohorts", isDirectory: true)
            try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
            let path = try store.writeCohortManifest(cohortId: cohort.id, dataRoot: dataRoot, path: out.appendingPathComponent("\(cohort.id).toml").path)
            reload()
            Task { await library.openCohort(URL(fileURLWithPath: path)) }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

// MARK: - Helpers

private func age(_ a: AnimalRow) -> String {
    guard let dob = a.dob ?? a.arrivedOn else { return "—" }
    let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd"; f.locale = Locale(identifier: "en_US_POSIX")
    guard let d = f.date(from: dob) else { return "—" }
    let days = Calendar.current.dateComponents([.day], from: d, to: Date()).day ?? 0
    return days < 70 ? "\(days) d" : "\(days / 7) wk"
}

private func sexSymbol(_ s: String) -> String { s == "M" ? "♂" : s == "F" ? "♀" : "?" }

private struct ErrorBanner: View {
    @Environment(LabModel.self) private var lab

    var body: some View {
        if let e = lab.error {
            Label(e, systemImage: "xmark.octagon.fill").foregroundStyle(.red).font(.callout)
                .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                .background(.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
                .onTapGesture { lab.error = nil }
        } else if let t = lab.toast {
            Label(t, systemImage: "info.circle.fill").foregroundStyle(.orange).font(.callout)
                .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                .background(.orange.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
                .onTapGesture { lab.toast = nil }
        }
    }
}

/// First run: nothing in the store yet.
private struct LabOnboarding: View {
    @Environment(LabModel.self) private var lab
    @State private var showProtocol = false

    var body: some View {
        ContentUnavailableView {
            Label("Set up your colony", systemImage: "square.grid.3x3.square")
        } description: {
            Text("Animals, cages, breeding, surgeries and sessions live in one local file on this Mac. Every change is kept in a tamper-evident history.")
        } actions: {
            HStack {
                Button("Add Protocol…") { showProtocol = true }.buttonStyle(.borderedProminent)
                Button("Load Demo Colony") { lab.seedDemo() }
            }
            .controlSize(.large)
        }
        .sheet(isPresented: $showProtocol) { ProtocolSheet() }
    }
}

// MARK: - Animals

struct AnimalsView: View {
    @Environment(LabModel.self) private var lab
    @Environment(Library.self) private var library
    @State private var search = ""
    @State private var selection: AnimalRow.ID?
    @State private var showNew = false
    @State private var surgeryFor: AnimalRow?

    private var filtered: [AnimalRow] {
        guard !search.isEmpty else { return lab.animals }
        return lab.animals.filter { "\($0.label) \(lab.line($0.lineId)) \(lab.cageLabel($0.id)) \($0.earTag ?? "")".localizedCaseInsensitiveContains(search) }
    }

    var body: some View {
        VStack(spacing: 0) {
            ErrorBanner().padding(.horizontal, 12).padding(.top, 8)
            if lab.protocols.isEmpty {
                LabOnboarding()
            } else {
                HSplitView {
                    VStack(spacing: 0) {
                    TextField("Search animal, line, cage, ear tag", text: $search)
                        .textFieldStyle(.roundedBorder)
                        .padding(10)
                    Table(filtered, selection: $selection) {
                        TableColumn("Animal") { a in
                            HStack(spacing: 6) {
                                Text(sexSymbol(a.sex)).foregroundStyle(a.sex == "M" ? .blue : .pink)
                                Text(a.label).fontWeight(.medium)
                                if a.deathDate != nil { Image(systemName: "xmark.circle").foregroundStyle(.secondary) }
                            }
                            .draggable(a.id)
                        }
                        TableColumn("Line") { Text(lab.line($0.lineId)) }
                        TableColumn("Age") { Text(age($0)).monospacedDigit() }.width(56)
                        TableColumn("Cage") { Text(lab.cageLabel($0.id)) }.width(64)
                        TableColumn("Sessions") { a in Text("\(lab.sessions.filter { $0.animalId == a.id }.count)").monospacedDigit() }.width(64)
                    }
                    .contextMenu(forSelectionType: AnimalRow.ID.self) { ids in
                        if let id = ids.first, let a = lab.animals.first(where: { $0.id == id }) {
                            Menu("Move to Cage") {
                                ForEach(lab.cages) { c in
                                    Button("\(c.label) (\(lab.occupants(c.id).count)/\(c.capacity))") { lab.act { try $0.moveAnimal(animalId: a.id, cageId: c.id, reason: "move") } }
                                }
                            }
                            Button("Record Surgery…") { surgeryFor = a }
                            Menu("Exclude from Analyses") {
                                ForEach(["fiber_misplaced", "no_expression", "signal_qc", "health", "procedure_failure", "other"], id: \.self) { r in
                                    Button(r.replacingOccurrences(of: "_", with: " ").capitalized) { lab.act { _ = try $0.exclude(scope: "animal", targetId: a.id, cohortId: nil, reason: r, detail: nil); return ["\(a.label) excluded: \(r.replacingOccurrences(of: "_", with: " "))"] } }
                                }
                            }
                        }
                    }
                    }
                    .frame(minWidth: 300, maxHeight: .infinity)
                    Group {
                        if let id = selection, let a = lab.animals.first(where: { $0.id == id }) {
                            AnimalTimeline(animal: a)
                        } else {
                            ContentUnavailableView("Select an animal", systemImage: "list.bullet.below.rectangle", description: Text("Its whole life appears here: cages, surgeries, sessions, notes."))
                        }
                    }
                    .frame(minWidth: 240, idealWidth: 360, maxHeight: .infinity)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle("Animals")
        .navigationSubtitle("\(lab.animals.filter { $0.deathDate == nil }.count) alive · \(lab.cages.count) cages")
        .toolbar {
            ToolbarItem { Button { showNew = true } label: { Label("New Animal", systemImage: "plus") }.disabled(lab.protocols.isEmpty).keyboardShortcut("n", modifiers: [.command, .option]) }
        }
        .sheet(isPresented: $showNew) { AnimalSheet() }
        .sheet(item: $surgeryFor) { SurgerySheet(animal: $0) }
    }
}

private struct AnimalTimeline: View {
    @Environment(LabModel.self) private var lab
    let animal: AnimalRow
    @State private var note = ""

    private func icon(_ kind: String) -> (String, Color) {
        switch kind {
        case "birth", "arrival": ("sparkles", .green)
        case "cage": ("square.grid.2x2", .secondary)
        case "genotype": ("allergens", .purple)
        case "procedure": ("cross.case", .red)
        case "session": ("waveform.path.ecg", .blue)
        case "exclusion": ("nosign", .orange)
        case "note": ("text.bubble", .teal)
        case "death": ("xmark.circle", .secondary)
        default: ("circle", .secondary)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text("\(sexSymbol(animal.sex)) \(animal.label)").font(.title2.weight(.semibold))
                Text("\(lab.line(animal.lineId)) · \(age(animal)) · cage \(lab.cageLabel(animal.id))\(animal.earTag.map { " · tag \($0)" } ?? "")").foregroundStyle(.secondary)
            }
            .padding(14)
            Divider()
            List {
                ForEach(Array(lab.timeline(animal.id).enumerated()), id: \.offset) { _, item in
                    let (sym, color) = icon(item.kind)
                    HStack(alignment: .top, spacing: 10) {
                        Image(systemName: sym).foregroundStyle(color).frame(width: 18)
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                Text(item.title.capitalized(with: nil)).fontWeight(.medium)
                                Spacer()
                                Text(item.when).font(.caption).foregroundStyle(.secondary).monospacedDigit()
                            }
                            if !item.detail.isEmpty { Text(item.detail).font(.callout).foregroundStyle(.secondary) }
                        }
                    }
                    .padding(.vertical, 2)
                }
            }
            .listStyle(.inset)
            HStack {
                TextField("Add a note…", text: $note).textFieldStyle(.roundedBorder)
                    .onSubmit(addNote)
                Button("Add", action: addNote).disabled(note.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding(10)
        }
    }

    private func addNote() {
        let body = note.trimmingCharacters(in: .whitespaces)
        guard !body.isEmpty else { return }
        lab.act { _ = try $0.insert(table: "note", json: json(["author": lab.initials, "body_md": body, "entity": "animal", "entity_id": animal.id])); return [] }
        note = ""
    }
}

// MARK: - Cages

struct CagesView: View {
    @Environment(LabModel.self) private var lab
    @State private var showNew = false
    @State private var target: String?

    var body: some View {
        VStack(spacing: 0) {
            ErrorBanner().padding(.horizontal, 12).padding(.top, 8)
            if lab.protocols.isEmpty {
                LabOnboarding()
            } else {
                ScrollView {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 190), spacing: 12)], spacing: 12) {
                        ForEach(lab.cages) { c in
                            CageCard(cage: c, highlighted: target == c.id)
                                .dropDestination(for: String.self) { ids, _ in
                                    for id in ids { lab.act { try $0.moveAnimal(animalId: id, cageId: c.id, reason: "move") } }
                                    return true
                                } isTargeted: { target = $0 ? c.id : (target == c.id ? nil : target) }
                        }
                    }
                    .padding(14)
                }
            }
        }
        .navigationTitle("Cages")
        .navigationSubtitle("Drag an animal onto a cage to move it")
        .toolbar {
            ToolbarItem { Button { showNew = true } label: { Label("New Cage", systemImage: "plus") }.disabled(lab.protocols.isEmpty) }
            ToolbarItem { Button { printCards(lab.cages) } label: { Label("Print Cards", systemImage: "printer") }.disabled(lab.cages.isEmpty).help("Cage cards with QR codes as a PDF") }
        }
        .sheet(isPresented: $showNew) { CageSheet() }
    }

    private func printCards(_ cages: [CageRow]) {
        let cards = cages.map { CardView(cage: $0, occupants: lab.occupants($0.id), line: { lab.line($0) }) }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("SimpleFP cage cards.pdf")
        guard let ctx = CGContext(url as CFURL, mediaBox: nil, nil) else { return }
        // 3 × 5 in index cards, one per page.
        var box = CGRect(x: 0, y: 0, width: 360, height: 216)
        for card in cards {
            let r = ImageRenderer(content: card.frame(width: box.width, height: box.height))
            r.render { _, draw in
                ctx.beginPage(mediaBox: &box)
                draw(ctx)
                ctx.endPage()
            }
        }
        ctx.closePDF()
        NSWorkspace.shared.open(url)
    }
}

private struct CageCard: View {
    @Environment(LabModel.self) private var lab
    let cage: CageRow
    let highlighted: Bool

    var body: some View {
        let occ = lab.occupants(cage.id)
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(cage.label).font(.headline)
                Text(cage.kind).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Text("\(occ.count)/\(cage.capacity)").font(.caption.monospacedDigit()).foregroundStyle(occ.count > cage.capacity ? .red : .secondary)
            }
            Gauge(value: Double(min(occ.count, cage.capacity)), in: 0...Double(cage.capacity)) {}
                .gaugeStyle(.accessoryLinearCapacity).tint(occ.count > cage.capacity ? .red : .accentColor)
            FlowChips(animals: occ)
            if occ.isEmpty { Text("Empty").font(.caption).foregroundStyle(.tertiary) }
            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(minHeight: 120, alignment: .top)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(highlighted ? Color.accentColor : .clear, lineWidth: 2))
        .animation(.snappy, value: highlighted)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Cage \(cage.label), \(occ.count) of \(cage.capacity)")
    }
}

private struct FlowChips: View {
    let animals: [AnimalRow]

    var body: some View {
        // Simple wrap: rows of 3 chips keep cards compact and scannable.
        VStack(alignment: .leading, spacing: 4) {
            ForEach(Array(stride(from: 0, to: animals.count, by: 3)), id: \.self) { i in
                HStack(spacing: 4) {
                    ForEach(animals[i..<min(i + 3, animals.count)]) { a in
                        Text("\(sexSymbol(a.sex)) \(a.label)")
                            .font(.caption.weight(.medium))
                            .padding(.horizontal, 7).padding(.vertical, 3)
                            .background((a.sex == "M" ? Color.blue : Color.pink).opacity(0.12), in: Capsule())
                            .draggable(a.id)
                    }
                }
            }
        }
    }
}

/// Printed cage card: large label, QR (UUID only, no personal or protocol data), key facts.
private struct CardView: View {
    let cage: CageRow
    let occupants: [AnimalRow]
    let line: (String?) -> String

    private var qr: Image {
        let f = CIFilter.qrCodeGenerator()
        f.message = Data("simplefp://cage/\(cage.id)".utf8)
        f.correctionLevel = "M"
        guard let out = f.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)),
              let cg = CIContext().createCGImage(out, from: out.extent) else { return Image(systemName: "qrcode") }
        return Image(decorative: cg, scale: 1).interpolation(.none)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text(cage.label).font(.system(size: 40, weight: .bold, design: .rounded))
                Text("\(cage.kind.capitalized) · \(cage.room ?? "") \(cage.rack ?? "")").font(.system(size: 11))
                Divider()
                ForEach(occupants) { a in
                    Text("\(sexSymbol(a.sex)) \(a.label)   \(line(a.lineId))   DOB \(a.dob ?? "—")").font(.system(size: 11, design: .monospaced))
                }
                Spacer()
            }
            VStack {
                qr.resizable().frame(width: 96, height: 96)
                Text("SimpleFP").font(.system(size: 8)).foregroundStyle(.secondary)
            }
        }
        .padding(18)
        .foregroundStyle(.black)
        .background(.white)
    }
}

// MARK: - Tasks and colony cohorts

struct TasksView: View {
    @Environment(LabModel.self) private var lab
    @Environment(Library.self) private var library

    private func icon(_ k: String) -> String {
        ["wean": "figure.and.child.holdinghands", "post_op": "cross.case", "protocol": "doc.badge.clock", "capacity": "exclamationmark.triangle", "genotype": "allergens"][k] ?? "circle"
    }

    var body: some View {
        List {
            ErrorBanner()
            Section("Due") {
                if lab.tasks.isEmpty { Text("Nothing due. Tasks come from the records: weaning, post-op checks, protocol expiry, capacity, genotyping.").foregroundStyle(.secondary) }
                ForEach(lab.tasks, id: \.self) { t in
                    HStack(spacing: 10) {
                        Image(systemName: icon(t.kind)).foregroundStyle(t.overdue ? .red : .accentColor).frame(width: 22)
                        Text(t.title)
                        Spacer()
                        Text(t.overdue ? "overdue · \(t.dueOn)" : t.dueOn).font(.callout.monospacedDigit()).foregroundStyle(t.overdue ? .red : .secondary)
                    }
                }
            }
            Section("Colony cohorts") {
                ForEach(lab.cohorts) { c in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(c.name).fontWeight(.medium)
                            if let q = c.question { Text(q).font(.callout).foregroundStyle(.secondary) }
                            if let h = c.membersHash { Text("Frozen · \(h.prefix(12))…").font(.caption.monospaced()).foregroundStyle(.secondary) }
                        }
                        Spacer()
                        Button(c.frozenAt == nil ? "Freeze & Analyze" : "Analyze") { lab.analyze(cohort: c, library: library) }
                            .help("Freezing locks membership and records its hash; the analysis cites it.")
                    }
                }
                if lab.cohorts.isEmpty { Text("No colony cohorts yet.").foregroundStyle(.secondary) }
            }
        }
        .navigationTitle("Tasks")
        .navigationSubtitle(today())
        .toolbar { ToolbarItem { Button { lab.reload() } label: { Label("Refresh", systemImage: "arrow.clockwise") } } }
    }
}

// MARK: - Sheets

private struct SheetFrame<Content: View>: View {
    @Environment(\.dismiss) private var dismiss
    let title: String
    let canSave: Bool
    let save: () -> Bool
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.title3.weight(.semibold))
            Form { content }.formStyle(.grouped)
            ErrorBanner()
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") { if save() { dismiss() } }.keyboardShortcut(.defaultAction).disabled(!canSave)
            }
        }
        .padding(20)
        .frame(width: 480)
    }
}

struct ProtocolSheet: View {
    @Environment(LabModel.self) private var lab
    @State private var number = ""
    @State private var title = ""
    @State private var from = Date()
    @State private var until = Calendar.current.date(byAdding: .year, value: 3, to: Date()) ?? Date()
    @State private var count = 100

    var body: some View {
        SheetFrame(title: "New protocol", canSave: !number.isEmpty && !title.isEmpty, save: save) {
            TextField("Protocol number", text: $number)
            TextField("Title", text: $title)
            DatePicker("Approved from", selection: $from, displayedComponents: .date)
            DatePicker("Expires on", selection: $until, displayedComponents: .date)
            Stepper("Approved animals: \(count)", value: $count, in: 1...10_000, step: 10)
        }
    }

    private func save() -> Bool {
        let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd"; f.locale = Locale(identifier: "en_US_POSIX")
        lab.act { _ = try $0.insert(table: "protocol", json: json(["number": number, "title": title, "approved_from": f.string(from: from), "expires_on": f.string(from: until), "species": "Mus musculus", "approved_count": count])); return [] }
        return lab.error == nil
    }
}

struct CageSheet: View {
    @Environment(LabModel.self) private var lab
    @State private var label = ""
    @State private var room = ""
    @State private var rack = ""
    @State private var kind = "holding"
    @State private var capacity = 5

    var body: some View {
        SheetFrame(title: "New cage", canSave: !label.isEmpty, save: save) {
            TextField("Label", text: $label)
            Picker("Type", selection: $kind) { ForEach(["holding", "breeding", "experimental", "quarantine"], id: \.self) { Text($0.capitalized).tag($0) } }
            Stepper("Capacity: \(capacity)", value: $capacity, in: 1...20)
            TextField("Room", text: $room)
            TextField("Rack", text: $rack)
        }
        .onAppear { label = "C-\(String(format: "%03d", lab.cages.count + 1))" }
    }

    private func save() -> Bool {
        guard let p = lab.protocols.first else { return false }
        lab.act { _ = try $0.insert(table: "cage", json: json(["label": label, "kind": kind, "capacity": capacity, "room": room.isEmpty ? nil : room, "rack": rack.isEmpty ? nil : rack, "protocol_id": p.id, "opened_on": today()])); return [] }
        return lab.error == nil
    }
}

struct AnimalSheet: View {
    @Environment(LabModel.self) private var lab
    @State private var label = ""
    @State private var vendor = "JAX"
    @State private var earTag = ""
    @State private var sex = "M"
    @State private var line: String?
    @State private var cage: String?
    @State private var dob = Calendar.current.date(byAdding: .day, value: -56, to: Date()) ?? Date()

    var body: some View {
        SheetFrame(title: "New animal", canSave: !label.isEmpty, save: save) {
            TextField("Label", text: $label)
            Picker("Sex", selection: $sex) { Text("♂ Male").tag("M"); Text("♀ Female").tag("F"); Text("Unknown").tag("U") }.pickerStyle(.segmented)
            Picker("Line", selection: $line) { Text("—").tag(String?.none); ForEach(lab.lines) { Text($0.name).tag(Optional($0.id)) } }
            DatePicker("Date of birth", selection: $dob, displayedComponents: .date)
            TextField("Vendor", text: $vendor)
            TextField("Ear tag", text: $earTag)
            Picker("Cage", selection: $cage) { Text("—").tag(String?.none); ForEach(lab.cages) { Text($0.label).tag(Optional($0.id)) } }
        }
        .onAppear { label = "A-\(String(format: "%03d", lab.animals.count + 1))" }
    }

    private func save() -> Bool {
        guard let p = lab.protocols.first else { return false }
        let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd"; f.locale = Locale(identifier: "en_US_POSIX")
        lab.act { store in
            let id = try store.insert(table: "animal", json: json(["label": label, "sex": sex, "line_id": line, "dob": f.string(from: dob), "source": "vendor", "vendor": vendor, "arrived_on": today(), "protocol_id": p.id, "ear_tag": earTag.isEmpty ? nil : earTag, "owner": lab.initials]))
            if let cage { return try store.moveAnimal(animalId: id, cageId: cage, reason: "arrival") }
            return []
        }
        return lab.error == nil
    }
}

/// One screen per surgery: frame, one injection and one fiber (the common photometry case).
struct SurgerySheet: View {
    @Environment(LabModel.self) private var lab
    let animal: AnimalRow
    @State private var weight = 25.0
    @State private var virus: String?
    @State private var region = "NAc core"
    @State private var hemi = "L"
    @State private var ap = 1.3
    @State private var ml = -1.0
    @State private var dv = 4.2
    @State private var volume = 300.0
    @State private var fiber = true
    @State private var fiberDV = 4.1
    @State private var core = 400.0
    @State private var na = 0.5

    var body: some View {
        SheetFrame(title: "Surgery · \(animal.label)", canSave: virus != nil && weight > 0, save: save) {
            Section("Animal") { LabeledContent("Weight before (g)") { TextField("", value: $weight, format: .number).multilineTextAlignment(.trailing) } }
            Section("Injection (bregma, skull-flat; AP + anterior, ML + right, DV from skull)") {
                Picker("Virus lot", selection: $virus) { Text("—").tag(String?.none); ForEach(lab.viruses) { Text($0.construct).tag(Optional($0.id)) } }
                TextField("Target region", text: $region)
                Picker("Hemisphere", selection: $hemi) { Text("Left").tag("L"); Text("Right").tag("R") }.pickerStyle(.segmented)
                coord("AP (mm)", $ap); coord("ML (mm)", $ml); coord("DV (mm)", $dv)
                coord("Volume (nl)", $volume)
            }
            Section("Optic fiber") {
                Toggle("Fiber implant at the same AP/ML", isOn: $fiber)
                if fiber { coord("Fiber DV (mm)", $fiberDV); coord("Core (µm)", $core); coord("NA", $na) }
            }
        }
        .onAppear { virus = lab.viruses.first?.id }
    }

    private func coord(_ t: String, _ v: Binding<Double>) -> some View {
        LabeledContent(t) { TextField("", value: v, format: .number.precision(.fractionLength(0...2))).multilineTextAlignment(.trailing).monospacedDigit() }
    }

    private func save() -> Bool {
        guard let p = lab.protocols.first, let virus else { return false }
        let body: [String: Any] = [
            "procedure": ["animal_id": animal.id, "performed_on": today(), "operators": lab.initials, "protocol_id": p.id, "weight_pre_g": weight],
            "frame": ["reference": "bregma", "skull_flat": 1],
            "injections": [["virus_lot_id": virus, "target_region": region, "hemisphere": hemi, "ap_mm": ap, "ml_mm": ml, "dv_mm": dv, "dv_reference": "skull", "volume_nl": volume]],
            "implants": fiber ? [["kind": "optic_fiber", "target_region": region, "hemisphere": hemi, "ap_mm": ap, "ml_mm": ml, "dv_mm": fiberDV, "dv_reference": "skull", "core_um": core, "na": na]] : [],
        ]
        guard let s = try? String(data: JSONSerialization.data(withJSONObject: body), encoding: .utf8) else { return false }
        lab.act { _ = try $0.recordSurgery(json: s); return ["Surgery recorded; post-op checks scheduled"] }
        return lab.error == nil
    }
}
