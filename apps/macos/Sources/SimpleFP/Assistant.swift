import Foundation
import Security
import SwiftUI

// MARK: - Provider (any OpenAI-compatible endpoint: local MLX/Ollama, Chutes TEE, Tinfoil, …)

struct ProviderPreset: Identifiable, Hashable {
    let id: String, name: String, baseURL: String, model: String, note: String
    static let all = [
        ProviderPreset(id: "local", name: "Local (this Mac)", baseURL: "http://127.0.0.1:8080/v1", model: "mlx-community/Qwen3-8B-4bit",
                       note: "Nothing leaves the Mac. Run an OpenAI-compatible server, e.g. `mlx_lm.server` or Ollama (http://127.0.0.1:11434/v1)."),
        ProviderPreset(id: "chutes", name: "Chutes (confidential compute)", baseURL: "https://llm.chutes.ai/v1", model: "",
                       note: "Models marked confidential_compute run in TEEs. Pick one of those; attestation is not verified by SimpleFP yet."),
        ProviderPreset(id: "tinfoil", name: "Tinfoil", baseURL: "https://inference.tinfoil.sh/v1", model: "",
                       note: "Attested enclaves. SimpleFP uses plain HTTPS here; full attestation needs Tinfoil's SDK (planned)."),
        ProviderPreset(id: "custom", name: "Custom", baseURL: "", model: "", note: "Any OpenAI-compatible /chat/completions endpoint."),
    ]
}

/// API keys live in the macOS Keychain, never in preferences or files.
enum Keychain {
    static func key(_ account: String) -> String? {
        var out: AnyObject?
        let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "SimpleFP.llm", kSecAttrAccount as String: account, kSecReturnData as String: true]
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data else { return nil }
        return String(data: d, encoding: .utf8)
    }

    static func set(_ account: String, _ value: String) {
        let base: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "SimpleFP.llm", kSecAttrAccount as String: account]
        SecItemDelete(base as CFDictionary)
        guard !value.isEmpty else { return }
        var add = base
        add[kSecValueData as String] = Data(value.utf8)
        SecItemAdd(add as CFDictionary, nil)
    }
}

struct LLMSettings {
    @AppStorage("llm.provider") var provider = "local"
    @AppStorage("llm.baseURL") var baseURL = "http://127.0.0.1:8080/v1"
    @AppStorage("llm.model") var model = "mlx-community/Qwen3-8B-4bit"

    var isLocal: Bool {
        guard let host = URL(string: baseURL)?.host else { return false }
        return ["127.0.0.1", "localhost", "::1"].contains(host)
    }
}

struct ChatMessage: Identifiable, Hashable {
    let id = UUID()
    let role: String
    let text: String
}

enum LLM {
    nonisolated static func complete(baseURL: String, model: String, key: String?, messages: [[String: String]]) async throws -> String {
        guard let url = URL(string: baseURL.trimmingCharacters(in: .whitespaces) + "/chat/completions"), ["http", "https"].contains(url.scheme ?? "") else {
            throw NSError(domain: "SimpleFP", code: 20, userInfo: [NSLocalizedDescriptionKey: "Set a valid provider URL in Settings."])
        }
        // Plain HTTP only to this Mac; anything remote must be HTTPS.
        if url.scheme == "http", !["127.0.0.1", "localhost", "::1"].contains(url.host ?? "") {
            throw NSError(domain: "SimpleFP", code: 21, userInfo: [NSLocalizedDescriptionKey: "Remote providers must use HTTPS."])
        }
        var req = URLRequest(url: url, timeoutInterval: 120)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let key, !key.isEmpty { req.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization") }
        req.httpBody = try JSONSerialization.data(withJSONObject: ["model": model, "messages": messages, "temperature": 0.2])
        let (data, resp) = try await URLSession.shared.data(for: req)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else {
            throw NSError(domain: "SimpleFP", code: code, userInfo: [NSLocalizedDescriptionKey: "Provider returned HTTP \(code): \(String(decoding: data.prefix(300), as: UTF8.self))"])
        }
        let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        let msg = ((obj?["choices"] as? [[String: Any]])?.first?["message"] as? [String: Any])?["content"] as? String
        return msg ?? "(empty reply)"
    }
}

// MARK: - Context: what the assistant may see (summaries only, never raw data)

extension Recording {
    var assistantContext: String {
        var s = "Recording: \(info.name), duration \(Int(info.durationS)) s, signal \(info.signal), control \(info.control ?? "none").\n"
        if let q = results?.qc {
            s += String(format: "QC: rate %.2f Hz; bleaching %.1f %% (signal)%@; control fit r %@ slope %@; noise %.3f %% dF/F; residual drift %+.2f %%/10 min; warnings: %@.\n",
                        q.fs, q.bleachSignalPct, q.bleachFallback ? " [fallback]" : "", q.motionR.map { String(format: "%.2f", $0) } ?? "n/a", q.motionSlope.map { String(format: "%.2f", $0) } ?? "n/a",
                        q.noiseDffPct, q.residualDriftPctPer10min, q.warnings.isEmpty ? "none" : q.warnings.joined(separator: "; "))
        }
        for e in results?.events ?? [] {
            let means = e.metrics.map(\.mean)
            let m = means.isEmpty ? 0 : means.reduce(0, +) / Double(means.count)
            s += String(format: "Event %@: %d trials (%d outside range); mean response %.2f (%@); control responds: %@.\n", e.label, e.eventTimes.count, e.dropped.count, m, e.unit, e.controlResponds ? "yes" : "no")
        }
        s += "Preset (TOML):\n" + presetTOML
        return s
    }
}

private let systemPrompt = """
You are the analysis assistant inside SimpleFP, a fiber photometry app (TDT recordings; GRAB-ACh3.0 and GCaMP sensors).
Ground every statement in the provided summary; never invent numbers, p-values or results that are not in it.
Single-recording statistics are descriptive (trials are not independent); say so when relevant.
When a parameter change would help, put each one on its own line exactly as: SET <dotted.key> = <TOML value>
(for example: SET preprocess.lowpass_hz = 6.0). Explain why in one sentence. Be concise.
"""

/// Ask questions about the current recording; suggested parameter changes become one-click, validated actions.
struct AssistantView: View {
    let rec: Recording
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openSettings) private var openSettings
    @State private var messages: [ChatMessage] = []
    @State private var input = ""
    @State private var busy = false
    @State private var error: String?
    private let settings = LLMSettings()

    private func suggestions(_ text: String) -> [(String, String)] {
        text.split(separator: "\n").compactMap { line in
            let l = line.trimmingCharacters(in: .whitespaces)
            guard l.hasPrefix("SET "), let eq = l.firstIndex(of: "=") else { return nil }
            let key = l[l.index(l.startIndex, offsetBy: 4)..<eq].trimmingCharacters(in: .whitespaces)
            let value = l[l.index(after: eq)...].trimmingCharacters(in: .whitespaces)
            return key.isEmpty || value.isEmpty ? nil : (key, value)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Label("Lab Assistant", systemImage: "sparkles").font(.title3.weight(.semibold))
                Spacer()
                Label(settings.isLocal ? "On this Mac" : URL(string: settings.baseURL)?.host ?? "remote", systemImage: settings.isLocal ? "lock.laptopcomputer" : "network")
                    .font(.caption).foregroundStyle(settings.isLocal ? .green : .orange)
                    .help(settings.isLocal ? "Requests stay on this Mac" : "Summaries (QC, preset, event statistics) are sent to this provider. Raw traces and video are never sent.")
                Button("Settings…") { openSettings() }
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10) {
                    if messages.isEmpty {
                        Text("Ask about this recording: “Is the motion correction appropriate?”, “Why is the control flagged?”, “What window should I use for feeding bouts?”. The assistant sees the analysis summary and preset, never raw data.")
                            .foregroundStyle(.secondary).font(.callout)
                    }
                    ForEach(messages) { m in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(m.role == "user" ? "You" : "Assistant").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                            Text(m.text).textSelection(.enabled)
                            ForEach(Array(suggestions(m.text).enumerated()), id: \.offset) { _, kv in
                                Button("Apply \(kv.0) = \(kv.1)") { rec.setParam(kv.0, kv.1) }
                                    .buttonStyle(.borderedProminent).controlSize(.small)
                                    .help("Validated by the engine and re-analyzed; undo with Reset in the inspector")
                            }
                        }
                        .padding(10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(m.role == "user" ? Color.accentColor.opacity(0.08) : Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
                    }
                    if busy { ProgressView().controlSize(.small) }
                }
            }
            if let error { Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).font(.callout) }
            if let e = rec.paramError { Label(e, systemImage: "xmark.octagon.fill").foregroundStyle(.red).font(.callout) }
            HStack {
                TextField("Ask about this recording…", text: $input, axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(1...4).onSubmit(send)
                Button("Send", action: send).keyboardShortcut(.return, modifiers: .command).disabled(input.trimmingCharacters(in: .whitespaces).isEmpty || busy)
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
        }
        .padding(18)
        .frame(width: 620, height: 560)
    }

    private func send() {
        let q = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty, !busy else { return }
        input = ""
        messages.append(ChatMessage(role: "user", text: q))
        busy = true
        error = nil
        var wire: [[String: String]] = [["role": "system", "content": systemPrompt + "\n\nCurrent analysis:\n" + rec.assistantContext]]
        wire += messages.map { ["role": $0.role, "content": $0.text] }
        let (base, model, key) = (settings.baseURL, settings.model, Keychain.key(settings.provider))
        Task {
            do {
                let reply = try await LLM.complete(baseURL: base, model: model, key: key, messages: wire)
                messages.append(ChatMessage(role: "assistant", text: reply))
            } catch {
                self.error = error.localizedDescription
            }
            busy = false
        }
    }
}

struct SettingsView: View {
    @AppStorage("llm.provider") private var provider = "local"
    @AppStorage("llm.baseURL") private var baseURL = "http://127.0.0.1:8080/v1"
    @AppStorage("llm.model") private var model = "mlx-community/Qwen3-8B-4bit"
    @AppStorage("labInitials") private var initials = ""
    @State private var key = ""

    var body: some View {
        Form {
            Section("You") {
                TextField("Initials (recorded on every change in the lab store)", text: $initials)
            }
            Section("AI provider") {
                Picker("Provider", selection: $provider) { ForEach(ProviderPreset.all) { Text($0.name).tag($0.id) } }
                    .onChange(of: provider) { _, id in
                        if let p = ProviderPreset.all.first(where: { $0.id == id }), id != "custom" { baseURL = p.baseURL; if !p.model.isEmpty { model = p.model } }
                        key = Keychain.key(id) ?? ""
                    }
                TextField("Base URL", text: $baseURL)
                TextField("Model", text: $model)
                SecureField("API key (stored in Keychain)", text: $key).onSubmit { Keychain.set(provider, key) }
                Button("Save Key") { Keychain.set(provider, key) }
                Text(ProviderPreset.all.first { $0.id == provider }?.note ?? "").font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 520)
        .onAppear { key = Keychain.key(provider) ?? "" }
    }
}
