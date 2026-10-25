import AVFoundation
import CoreImage
import CoreML
import SwiftUI

/// Sidecar `<model>.pose.json` describing a Core ML pose model's I/O.
struct PoseConfig: Codable, Sendable {
    /// Input feature name; an image input or a `[1, 3, H, W]` multi-array.
    let input: String
    let width: Int
    let height: Int
    /// Heatmap output feature name: `[1, K, h, w]` (or `[1, h, w, K]` with `channels_last`).
    let output: String
    let channelsLast: Bool
    let bodyparts: [String]
    /// Square crop around the tracked animal, in source-video pixels (`nil` = whole frame).
    let cropPx: Int?
    /// Multiply 0–255 pixel values by this for multi-array inputs (e.g. 1/255).
    let inputScale: Double?

    enum CodingKeys: String, CodingKey {
        case input, width, height, output, bodyparts
        case channelsLast = "channels_last", cropPx = "crop_px", inputScale = "input_scale"
    }
}

/// Runs a Core ML pose model over a video, centred on the tracked animal. Models are the
/// user's own (or downloaded under their license); SimpleFP ships no pose weights.
enum PoseRunner {
    /// Loads `.mlpackage`/`.mlmodel` (compiling once) or `.mlmodelc`, plus its `.pose.json`.
    nonisolated static func load(_ url: URL) async throws -> (MLModel, PoseConfig) {
        let cfgURL = url.deletingPathExtension().appendingPathExtension("pose.json")
        guard let data = try? Data(contentsOf: cfgURL) else {
            throw NSError(domain: "SimpleFP", code: 40, userInfo: [NSLocalizedDescriptionKey: "Missing \(cfgURL.lastPathComponent) describing the model's input, output and bodyparts."])
        }
        let cfg = try JSONDecoder().decode(PoseConfig.self, from: data)
        let compiled = url.pathExtension == "mlmodelc" ? url : try await MLModel.compileModel(at: url)
        let mc = MLModelConfiguration()
        mc.computeUnits = .all // Neural Engine when the model supports it
        return (try MLModel(contentsOf: compiled, configuration: mc), cfg)
    }

    /// Keypoints per frame (flattened x, y, confidence in source pixels), sequentially decoded.
    /// `center(k)` gives the tracked animal's position in source pixels (nil = lost frame).
    nonisolated static func run(model: MLModel, cfg: PoseConfig, video: URL, center: @Sendable (Int) -> CGPoint?, progress: @Sendable (Double) -> Void) async throws -> (xyc: [Double], width: Int, height: Int) {
        let asset = AVURLAsset(url: video)
        guard let track = try await asset.loadTracks(withMediaType: .video).first else { throw NSError(domain: "SimpleFP", code: 41) }
        let size = try await track.load(.naturalSize)
        let total = max(1, Int(try await track.load(.nominalFrameRate) * Float(try await asset.load(.duration).seconds)))
        let reader = try AVAssetReader(asset: asset)
        let out = AVAssetReaderTrackOutput(track: track, outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
        out.alwaysCopiesSampleData = false
        reader.add(out)
        guard reader.startReading() else { throw reader.error ?? NSError(domain: "SimpleFP", code: 42) }
        let ci = CIContext(options: [.useSoftwareRenderer: false])
        let isImage = model.modelDescription.inputDescriptionsByName[cfg.input]?.type == .image
        let k = cfg.bodyparts.count
        var xyc: [Double] = []
        var i = 0
        var input: CVPixelBuffer?
        CVPixelBufferCreate(nil, cfg.width, cfg.height, kCVPixelFormatType_32BGRA, [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary, &input)
        guard let input else { throw NSError(domain: "SimpleFP", code: 43) }
        while let sb = out.copyNextSampleBuffer() {
            defer { i += 1 }
            guard let pb = CMSampleBufferGetImageBuffer(sb) else { continue }
            // Crop square around the animal (clamped to the frame), else the whole frame.
            let (fw, fh) = (Double(size.width), Double(size.height))
            var crop = CGRect(x: 0, y: 0, width: fw, height: fh)
            if let s = cfg.cropPx.map(Double.init) {
                guard let c = center(i) else { xyc += Array(repeating: .nan, count: 3 * k); continue }
                crop = CGRect(x: min(max(c.x - s / 2, 0), fw - s), y: min(max(c.y - s / 2, 0), fh - s), width: s, height: s)
            }
            // Core Image's origin is bottom-left; video rows run top-down.
            let img = CIImage(cvPixelBuffer: pb)
                .cropped(to: CGRect(x: crop.minX, y: fh - crop.maxY, width: crop.width, height: crop.height))
                .transformed(by: CGAffineTransform(translationX: -crop.minX, y: -(fh - crop.maxY)))
                .transformed(by: CGAffineTransform(scaleX: Double(cfg.width) / crop.width, y: Double(cfg.height) / crop.height))
            ci.render(img, to: input)
            let feature: MLFeatureValue
            if isImage {
                feature = MLFeatureValue(pixelBuffer: input)
            } else {
                let arr = try MLMultiArray(shape: [1, 3, NSNumber(value: cfg.height), NSNumber(value: cfg.width)], dataType: .float32)
                CVPixelBufferLockBaseAddress(input, .readOnly)
                let base = CVPixelBufferGetBaseAddress(input)!.assumingMemoryBound(to: UInt8.self)
                let stride = CVPixelBufferGetBytesPerRow(input), scale = Float(cfg.inputScale ?? 1)
                let dst = arr.dataPointer.assumingMemoryBound(to: Float.self)
                let plane = cfg.width * cfg.height
                for y in 0..<cfg.height {
                    for x in 0..<cfg.width {
                        let p = base + y * stride + x * 4 // BGRA
                        let o = y * cfg.width + x
                        dst[o] = Float(p[2]) * scale; dst[plane + o] = Float(p[1]) * scale; dst[2 * plane + o] = Float(p[0]) * scale
                    }
                }
                CVPixelBufferUnlockBaseAddress(input, .readOnly)
                feature = MLFeatureValue(multiArray: arr)
            }
            let pred = try await model.prediction(from: MLDictionaryFeatureProvider(dictionary: [cfg.input: feature]))
            guard let hm = pred.featureValue(for: cfg.output)?.multiArrayValue else { throw NSError(domain: "SimpleFP", code: 44, userInfo: [NSLocalizedDescriptionKey: "model has no '\(cfg.output)' output"]) }
            let shape = hm.shape.map(\.intValue)
            let (hh, ww) = cfg.channelsLast ? (shape[shape.count - 3], shape[shape.count - 2]) : (shape[shape.count - 2], shape[shape.count - 1])
            let n = k * hh * ww
            var bytes = Data(count: n * 4)
            bytes.withUnsafeMutableBytes { raw in
                let d = raw.bindMemory(to: Float.self)
                if hm.dataType == .float32 { let s = hm.dataPointer.assumingMemoryBound(to: Float.self); for j in 0..<n { d[j] = s[j] } } else { for j in 0..<n { d[j] = hm[j].floatValue } }
            }
            xyc += try decodePoseHeatmaps(data: bytes, k: UInt32(k), h: UInt32(hh), w: UInt32(ww), channelsLast: cfg.channelsLast, crop: [crop.minX, crop.minY, crop.maxX, crop.maxY])
            if i % 200 == 0 { progress(Double(i) / Double(total)) }
        }
        if reader.status == .failed { throw reader.error ?? NSError(domain: "SimpleFP", code: 45) }
        return (xyc, Int(size.width), Int(size.height))
    }
}

extension VideoModel {
    /// Run a pose model centred on the tracked animal, then show keypoints and save a DLC-style CSV next to the block.
    func runPose(_ modelURL: URL) async {
        guard let url = playableURL, !trackX.isEmpty, let rec = recording, let tr = track else { error = "Track the animal first: pose runs around the tracked position."; return }
        poseBusy = true
        defer { poseBusy = false }
        do {
            let (model, cfg) = try await PoseRunner.load(modelURL)
            let asset = AVURLAsset(url: url)
            let size = try await asset.loadTracks(withMediaType: .video).first?.load(.naturalSize) ?? .zero
            let (sx, sy) = (Double(size.width) / Double(tr.width), Double(size.height) / Double(tr.height))
            let xs = trackX, ys = trackY
            let center: @Sendable (Int) -> CGPoint? = { k in k < xs.count && xs[k].isFinite ? CGPoint(x: xs[k] * sx, y: ys[k] * sy) : nil }
            let result = try await PoseRunner.run(model: model, cfg: cfg, video: url, center: center) { _ in }
            pose = (cfg.bodyparts, result.xyc, result.width, result.height)
            try tracker?.setPose(bodyparts: cfg.bodyparts, xyc: result.xyc, sourceWidth: UInt32(result.width), sourceHeight: UInt32(result.height))
            let csv = try poseCsv(bodyparts: cfg.bodyparts, xyc: result.xyc, scorer: modelURL.deletingPathExtension().lastPathComponent)
            let dir = rec.url.appendingPathComponent("simplefp/pose", isDirectory: true)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try csv.write(to: dir.appendingPathComponent("\(camera)_\(modelURL.deletingPathExtension().lastPathComponent).csv"), atomically: true, encoding: .utf8)
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Keypoints of the current frame in source pixels.
    var currentKeypoints: [(String, CGPoint, Double)] {
        guard let p = pose else { return [] }
        let k = p.bodyparts.count
        let o = frame * k * 3
        guard o + 3 * k <= p.xyc.count else { return [] }
        return (0..<k).compactMap { b in
            let (x, y, c) = (p.xyc[o + 3 * b], p.xyc[o + 3 * b + 1], p.xyc[o + 3 * b + 2])
            return x.isFinite ? (p.bodyparts[b], CGPoint(x: x, y: y), c) : nil
        }
    }
}
