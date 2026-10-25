import AVFoundation
import AVKit
import SwiftUI

// MARK: - Frame-accurate timing

/// Presentation times of every video frame in movie time, from the sample table
/// (no decoding), mapped through the track's edit list. Seeking to these exact
/// CMTimes with zero tolerance lands on the intended frame (docs/research/video-ai.md §6).
struct FrameTable: Sendable {
    let movieTimes: [CMTime]

    static func load(_ url: URL) async throws -> FrameTable {
        let asset = AVURLAsset(url: url)
        guard let track = try await asset.loadTracks(withMediaType: .video).first else {
            throw NSError(domain: "SimpleFP", code: 1, userInfo: [NSLocalizedDescriptionKey: "\(url.lastPathComponent) has no video track"])
        }
        let segments = try await track.load(.segments)
        guard let cursor = track.makeSampleCursorAtFirstSampleInDecodeOrder() else {
            throw NSError(domain: "SimpleFP", code: 2, userInfo: [NSLocalizedDescriptionKey: "\(url.lastPathComponent): sample table unavailable (unsupported container?)"])
        }
        var pts: [CMTime] = []
        repeat { pts.append(cursor.presentationTimeStamp) } while cursor.stepInDecodeOrder(byCount: 1) == 1
        pts.sort { CMTimeCompare($0, $1) < 0 }
        // Media → movie time through edit segments (B-frame files start media PTS > 0).
        let mapped = pts.map { t -> CMTime in
            for s in segments where !s.isEmpty {
                let m = s.timeMapping
                if CMTimeRangeContainsTime(m.source, time: t) {
                    return CMTimeAdd(m.target.start, CMTimeSubtract(t, m.source.start))
                }
            }
            return t
        }
        return FrameTable(movieTimes: mapped)
    }

    /// Index of the frame on screen at movie time `t`.
    func frame(at t: CMTime) -> Int {
        var lo = 0, hi = movieTimes.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if CMTimeCompare(movieTimes[mid], t) <= 0 { lo = mid + 1 } else { hi = mid }
        }
        return max(0, lo - 1)
    }
}

// MARK: - Import (CamHal AVI → cached MP4)

/// Synapse's CamHal records DivX AVI, which AVFoundation cannot decode. Transcode
/// once into a cache (never next to the raw data) with every frame kept and no
/// B-frames, then verify the frame count matches the original exactly.
enum VideoImport {
    nonisolated static func tool(_ name: String) -> String? {
        ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map { "\($0)/\(name)" }.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    nonisolated static func run(_ exe: String, _ args: [String]) throws -> String {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: exe)
        p.arguments = args
        let out = Pipe()
        p.standardOutput = out
        p.standardError = Pipe()
        try p.run()
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { throw NSError(domain: "SimpleFP", code: Int(p.terminationStatus), userInfo: [NSLocalizedDescriptionKey: "\((exe as NSString).lastPathComponent) failed (\(p.terminationStatus))"]) }
        return String(decoding: data, as: UTF8.self)
    }

    nonisolated static func frames(_ ffprobe: String, _ url: URL) throws -> Int {
        let s = try run(ffprobe, ["-v", "error", "-select_streams", "v:0", "-count_packets", "-show_entries", "stream=nb_read_packets", "-of", "csv=p=0", url.path])
        guard let n = Int(s.trimmingCharacters(in: .whitespacesAndNewlines)) else { throw NSError(domain: "SimpleFP", code: 5, userInfo: [NSLocalizedDescriptionKey: "could not count frames in \(url.lastPathComponent)"]) }
        return n
    }

    /// A URL AVFoundation can play frame-accurately.
    nonisolated static func playable(_ url: URL) async throws -> URL {
        guard url.pathExtension.lowercased() == "avi" else { return url }
        let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
        let key = "\(url.path)|\((attrs[.size] as? Int) ?? 0)|\((attrs[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0)"
        let name = String(key.utf8.reduce(UInt64(0xcbf2_9ce4_8422_2325)) { ($0 ^ UInt64($1)) &* 0x100_0000_01b3 }, radix: 16)
        let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("SimpleFP/video", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let out = dir.appendingPathComponent("\(name).mp4")
        if FileManager.default.fileExists(atPath: out.path) { return out }
        guard let ffmpeg = tool("ffmpeg"), let ffprobe = tool("ffprobe") else {
            throw NSError(domain: "SimpleFP", code: 6, userInfo: [NSLocalizedDescriptionKey: "This block's camera video is a CamHal AVI (DivX), which macOS cannot play. Install ffmpeg (brew install ffmpeg) and reopen the block."])
        }
        let tmp = dir.appendingPathComponent("\(name).partial.mp4")
        _ = try run(ffmpeg, ["-v", "error", "-y", "-i", url.path, "-map", "0:v:0", "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-g", "10", "-bf", "0", "-fps_mode", "passthrough", "-an", "-movflags", "+faststart", tmp.path])
        let (a, b) = (try frames(ffprobe, url), try frames(ffprobe, tmp))
        guard a == b else {
            try? FileManager.default.removeItem(at: tmp)
            throw NSError(domain: "SimpleFP", code: 7, userInfo: [NSLocalizedDescriptionKey: "Transcoding changed the frame count (\(a) → \(b)); refusing to use it."])
        }
        try FileManager.default.moveItem(at: tmp, to: out)
        return out
    }
}

// MARK: - Model

@MainActor @Observable
final class VideoModel {
    let camera: String
    let url: URL
    let player: AVPlayer
    var table: FrameTable?
    /// Block-clock time of each frame from the camera epoc (frames beyond it are unclocked).
    var frameTimes: [Double] = []
    var clockHash = ""
    var frame = 0
    var playing = false
    var warning: String?
    var error: String?

    var ethogram: Ethogram
    var annotations: [AnnotationJSON] = []
    var openBouts: [String: Double] = [:]
    var scorer: String
    var dirty = false
    private var observer: Any?
    private weak var rec: Recording?

    init(rec: Recording, file: VideoFile, scorer: String) {
        self.rec = rec
        camera = file.camera
        url = URL(fileURLWithPath: file.path)
        player = AVPlayer()
        self.scorer = scorer
        ethogram = (try? JSONDecoder().decode(Ethogram.self, from: Data(defaultEthogram().utf8))) ?? Ethogram(name: "", behavior: [])
    }

    func load() async {
        guard let rec else { return }
        do {
            let clock = try rec.session.frameClock(camera: camera)
            frameTimes = clock.times.withUnsafeBytes { Array($0.bindMemory(to: Double.self)) }
            clockHash = clock.hash
            if url.pathExtension.lowercased() == "avi" { warning = "Preparing CamHal AVI for playback (one-time transcode)…" }
            let source = url
            let playable = try await Task.detached { try await VideoImport.playable(source) }.value
            warning = nil
            player.replaceCurrentItem(with: AVPlayerItem(url: playable))
            let t = try await FrameTable.load(playable)
            table = t
            warning = try rec.session.checkVideoFrames(camera: camera, videoFrames: UInt64(t.movieTimes.count))
            if let saved = try rec.session.loadAnnotations(scorer: scorer), let set = try? JSONDecoder().decode(AnnotationSetJSON.self, from: Data(saved.utf8)) {
                annotations = set.annotations
                ethogram = set.ethogram
            }
            observer = player.addPeriodicTimeObserver(forInterval: CMTime(value: 1, timescale: 60), queue: .main) { [weak self] time in
                MainActor.assumeIsolated { self?.tick(time) }
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func tick(_ time: CMTime) {
        guard let table else { return }
        let k = table.frame(at: time)
        if k != frame { frame = k }
        playing = player.rate != 0
        if let t = blockTime, let rec, playing || rec.cursor != t {
            rec.cursor = t
            // Keep the playhead in view while playing: page the trace forward.
            let span = rec.visible.upperBound - rec.visible.lowerBound
            if playing, !rec.visible.contains(t), span < rec.info.durationS {
                let lo = min(max(t - 0.1 * span, 0), rec.info.durationS - span)
                rec.visible = lo...(lo + span)
            }
        }
    }

    var blockTime: Double? { frame < frameTimes.count ? frameTimes[frame] : nil }
    var frameCount: Int { table?.movieTimes.count ?? 0 }

    func seek(frame k: Int) {
        guard let table, !table.movieTimes.isEmpty else { return }
        let k = min(max(k, 0), table.movieTimes.count - 1)
        frame = k
        player.seek(to: table.movieTimes[k], toleranceBefore: .zero, toleranceAfter: .zero)
        if let t = blockTime { rec?.cursor = t }
    }

    /// Show the frame on screen at block time `t` (e.g. a click on the trace).
    func seek(blockTime t: Double) {
        guard !frameTimes.isEmpty else { return }
        var lo = 0, hi = frameTimes.count
        while lo < hi { let m = (lo + hi) / 2; if frameTimes[m] <= t { lo = m + 1 } else { hi = m } }
        seek(frame: max(0, lo - 1))
    }

    func togglePlay() {
        if player.rate == 0 { player.play() } else { player.pause() }
        playing = player.rate != 0
    }

    /// Hotkey: point behaviors mark an instant; state behaviors toggle a bout
    /// (starting one stops any open bout in the same exclusive group).
    func mark(_ b: BehaviorJSON) {
        guard let t = blockTime else { warning = "This frame has no camera timestamp; scoring is disabled here."; return }
        func close(_ label: String, at end: Double) {
            if let s = openBouts.removeValue(forKey: label), end > s {
                annotations.append(AnnotationJSON(label: label, startS: s, endS: end, scorer: scorer, source: "manual", model: nil, confidence: nil, review: "accepted"))
            }
        }
        if b.kind == "point" {
            annotations.append(AnnotationJSON(label: b.label, startS: t, endS: nil, scorer: scorer, source: "manual", model: nil, confidence: nil, review: "accepted"))
        } else if openBouts[b.label] != nil {
            close(b.label, at: t)
        } else {
            if let g = b.exclusive { for o in ethogram.behavior where o.exclusive == g && o.label != b.label { close(o.label, at: t) } }
            openBouts[b.label] = t
        }
        annotations.sort { $0.startS < $1.startS }
        dirty = true
    }

    func delete(_ a: AnnotationJSON) {
        annotations.removeAll { $0 == a }
        dirty = true
    }

    var setJSON: String? {
        let set = AnnotationSetJSON(schema: 1, scorer: scorer, camera: camera, clockHash: clockHash, ethogram: ethogram, annotations: annotations)
        let enc = JSONEncoder()
        enc.keyEncodingStrategy = .convertToSnakeCase
        return (try? enc.encode(set)).flatMap { String(data: $0, encoding: .utf8) }
    }

    func save() {
        guard let rec, let json = setJSON else { return }
        do {
            _ = try rec.session.saveAnnotations(json: json)
            dirty = false
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Use the scored behaviors as analysis events (bout onsets, `_offset` for ends).
    func useAsEvents() {
        guard let rec, let json = setJSON else { return }
        save()
        do {
            rec.manualEvents = try annotationEvents(json: json)
            Task { await rec.run() }
        } catch {
            self.error = error.localizedDescription
        }
    }

    func stop() {
        if let observer { player.removeTimeObserver(observer) }
        player.pause()
    }
}

// Mirrors of the engine's JSON (snake_case on the wire).
struct BehaviorJSON: Codable, Hashable { let label: String; let key: String; let kind: String; let exclusive: String? }
struct Ethogram: Codable, Hashable { let name: String; let behavior: [BehaviorJSON] }
struct AnnotationJSON: Codable, Hashable {
    let label: String; let startS: Double; let endS: Double?; let scorer: String; let source: String; let model: String?; let confidence: Double?; let review: String
    enum CodingKeys: String, CodingKey { case label, startS = "start_s", endS = "end_s", scorer, source, model, confidence, review }
}
struct AnnotationSetJSON: Codable {
    let schema: Int; let scorer: String; let camera: String; let clockHash: String; let ethogram: Ethogram; let annotations: [AnnotationJSON]
    enum CodingKeys: String, CodingKey { case schema, scorer, camera, clockHash = "clock_hash", ethogram, annotations }
}

// MARK: - Views

private struct PlayerLayerView: NSViewRepresentable {
    let player: AVPlayer
    func makeNSView(context: Context) -> AVPlayerView {
        let v = AVPlayerView()
        v.player = player
        v.controlsStyle = .none
        v.videoGravity = .resizeAspect
        return v
    }
    func updateNSView(_ v: AVPlayerView, context: Context) { v.player = player }
}

struct VideoPane: View {
    @Bindable var model: VideoModel
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 8) {
            ZStack(alignment: .topLeading) {
                PlayerLayerView(player: model.player)
                    .background(.black)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                if !model.openBouts.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(model.openBouts.keys.sorted(), id: \.self) { l in
                            Label(l.replacingOccurrences(of: "_", with: " "), systemImage: "record.circle").font(.caption.weight(.semibold))
                                .padding(.horizontal, 8).padding(.vertical, 3).background(.red.opacity(0.85), in: Capsule()).foregroundStyle(.white)
                        }
                    }
                    .padding(8)
                }
            }
            transport
            ethogramBar
            if let w = model.warning { Label(w, systemImage: "exclamationmark.triangle.fill").font(.caption).foregroundStyle(.orange) }
            if let e = model.error { Label(e, systemImage: "xmark.octagon.fill").font(.caption).foregroundStyle(.red) }
            annotationList
        }
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(.space) { model.togglePlay(); return .handled }
        .onKeyPress(.leftArrow) { model.seek(frame: model.frame - 1); return .handled }
        .onKeyPress(.rightArrow) { model.seek(frame: model.frame + 1); return .handled }
        .onKeyPress(characters: .alphanumerics) { press in
            guard let b = model.ethogram.behavior.first(where: { $0.key.lowercased() == press.characters.lowercased() }) else { return .ignored }
            model.mark(b)
            return .handled
        }
        .onTapGesture { focused = true }
        .onAppear { focused = true }
    }

    private var transport: some View {
        HStack(spacing: 10) {
            Button { model.seek(frame: model.frame - 1) } label: { Image(systemName: "backward.frame.fill") }.help("Previous frame (←)")
            Button { model.togglePlay() } label: { Image(systemName: model.playing ? "pause.fill" : "play.fill") }.help("Play / pause (space)")
            Button { model.seek(frame: model.frame + 1) } label: { Image(systemName: "forward.frame.fill") }.help("Next frame (→)")
            Text("frame \(model.frame + 1) / \(model.frameCount)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
            Spacer()
            Text(model.blockTime.map { "t = " + String(format: "%.3f s", $0) } ?? "unclocked frame").font(.caption.monospacedDigit())
                .foregroundStyle(model.blockTime == nil ? .orange : .primary)
                .help("Block-clock time from the \(model.camera) camera epoc")
        }
        .buttonStyle(.borderless)
    }

    private var ethogramBar: some View {
        // Full labels when there is room; otherwise key caps with the label as a tooltip.
        ViewThatFits(in: .horizontal) {
            ethogramButtons(labels: true)
            ethogramButtons(labels: false)
        }
    }

    private func ethogramButtons(labels: Bool) -> some View {
        HStack(spacing: 6) {
            ForEach(model.ethogram.behavior, id: \.label) { b in
                let open = model.openBouts[b.label] != nil
                Button { model.mark(b) } label: {
                    HStack(spacing: 4) {
                        Text(b.key.uppercased()).font(.caption2.monospaced().weight(.bold)).padding(.horizontal, 4).background(.quaternary, in: RoundedRectangle(cornerRadius: 3))
                        if labels { Text(b.label.replacingOccurrences(of: "_", with: " ")).font(.caption).lineLimit(1) }
                        if b.kind == "state" { Image(systemName: open ? "stop.circle.fill" : "circle").font(.caption2).foregroundStyle(open ? .red : .secondary) }
                    }
                }
                .buttonStyle(.bordered).controlSize(.small)
                .help("\(b.label.replacingOccurrences(of: "_", with: " ")) (\(b.kind == "state" ? "bout: press to start, again to stop" : "point event")) — key \(b.key.uppercased())")
            }
        }
        .fixedSize()
    }

    private var annotationList: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("\(model.annotations.count) scored · \(model.scorer)").font(.caption.weight(.medium)).foregroundStyle(.secondary)
                Spacer()
                Button("Save") { model.save() }.disabled(!model.dirty).keyboardShortcut("s")
                Button("Analyze These Events") { model.useAsEvents() }.disabled(model.annotations.isEmpty)
                    .help("Align ΔF/F to the scored behaviors (bout onsets; ends as <label>_offset)")
            }
            .controlSize(.small)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(model.annotations, id: \.self) { a in
                        HStack {
                            Text(a.label.replacingOccurrences(of: "_", with: " ")).font(.caption.weight(.medium))
                            Text(a.endS.map { String(format: "%.2f–%.2f s", a.startS, $0) } ?? String(format: "%.2f s", a.startS)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                            Spacer()
                            Button { model.seek(blockTime: a.startS) } label: { Image(systemName: "arrow.right.circle") }.buttonStyle(.borderless).help("Jump to")
                            Button { model.delete(a) } label: { Image(systemName: "trash") }.buttonStyle(.borderless).help("Delete")
                        }
                    }
                }
            }
            .frame(maxHeight: 110)
        }
    }
}

// MARK: - Demo video

/// Render a synthetic top-down arena video whose frames match a camera clock: a
/// "mouse" wanders and visits the food port shortly before each event. Frame k is
/// shown at movie time k/fps; the camera epoc gives its block time.
enum DemoVideo {
    nonisolated static func write(to url: URL, frames: Int, fps: Int32, frameTimes: [Double], events: [Double]) async throws {
        try? FileManager.default.removeItem(at: url)
        let (w, h) = (320, 240)
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: w, AVVideoHeightKey: h,
            AVVideoCompressionPropertiesKey: [AVVideoMaxKeyFrameIntervalKey: 15, AVVideoAllowFrameReorderingKey: false],
        ])
        input.expectsMediaDataInRealTime = false
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA, kCVPixelBufferWidthKey as String: w, kCVPixelBufferHeightKey as String: h])
        writer.add(input)
        guard writer.startWriting() else { throw writer.error ?? NSError(domain: "SimpleFP", code: 3) }
        writer.startSession(atSourceTime: .zero)
        let food = CGPoint(x: 270, y: 120)
        for k in 0..<frames {
            while !input.isReadyForMoreMediaData { try await Task.sleep(for: .milliseconds(2)) }
            guard let pool = adaptor.pixelBufferPool else { break }
            var pb: CVPixelBuffer?
            CVPixelBufferPoolCreatePixelBuffer(nil, pool, &pb)
            guard let pb else { continue }
            CVPixelBufferLockBaseAddress(pb, [])
            let ctx = CGContext(data: CVPixelBufferGetBaseAddress(pb), width: w, height: h, bitsPerComponent: 8, bytesPerRow: CVPixelBufferGetBytesPerRow(pb), space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)!
            ctx.setFillColor(CGColor(gray: 0.86, alpha: 1)); ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
            ctx.setStrokeColor(CGColor(gray: 0.5, alpha: 1)); ctx.setLineWidth(4); ctx.stroke(CGRect(x: 10, y: 10, width: w - 20, height: h - 20))
            ctx.setFillColor(CGColor(red: 0.9, green: 0.6, blue: 0.2, alpha: 1)); ctx.fillEllipse(in: CGRect(x: food.x - 12, y: food.y - 12, width: 24, height: 24))
            let t = k < frameTimes.count ? frameTimes[k] : (frameTimes.last ?? 0) + Double(k - frameTimes.count + 1) / Double(fps)
            // Wander on a smooth path; within [e−2, e+4] s of an event, go to (and stay at) the food.
            var p = CGPoint(x: 120 + 70 * sin(t * 0.31) + 30 * sin(t * 1.3), y: 120 + 70 * cos(t * 0.23) + 20 * sin(t * 0.9))
            if let e = events.min(by: { abs($0 - t) < abs($1 - t) }), t > e - 2, t < e + 6 {
                let f = t < e ? CGFloat((t - (e - 2)) / 2) : t < e + 4 ? 1 : CGFloat(1 - (t - e - 4) / 2)
                p = CGPoint(x: p.x + (food.x - 22 - p.x) * f, y: p.y + (food.y - p.y) * f)
            }
            ctx.setFillColor(CGColor(gray: 0.15, alpha: 1)); ctx.fillEllipse(in: CGRect(x: p.x - 9, y: p.y - 6, width: 18, height: 12))
            ctx.setFillColor(CGColor(gray: 0.3, alpha: 1)); ctx.fillEllipse(in: CGRect(x: p.x + 6, y: p.y - 3, width: 6, height: 6))
            CVPixelBufferUnlockBaseAddress(pb, [])
            adaptor.append(pb, withPresentationTime: CMTime(value: CMTimeValue(k), timescale: fps))
        }
        input.markAsFinished()
        await writer.finishWriting()
        if writer.status != .completed { throw writer.error ?? NSError(domain: "SimpleFP", code: 4) }
    }
}
