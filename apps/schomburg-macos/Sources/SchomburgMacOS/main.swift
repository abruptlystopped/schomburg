import AppKit
import Combine
import Foundation
import SwiftUI

enum HostClientError: LocalizedError {
    case executableMissing, launchFailed, protocolFailure(String), operation(String)
    var errorDescription: String? {
        switch self {
        case .executableMissing: "Schomburg host executable was not found."
        case .launchFailed: "Schomburg host could not start."
        case .protocolFailure(let message), .operation(let message): message
        }
    }
}

struct HostResponse: Decodable {
    let protocolVersion: Int
    let id: JSONValue
    let ok: Bool
    let result: JSONValue?
    let error: HostProtocolError?
    enum CodingKeys: String, CodingKey { case protocolVersion = "protocol_version", id, ok, result, error }
}
struct HostProtocolError: Decodable { let code: String; let message: String }
enum JSONValue: Codable, Equatable {
    case string(String), number(Double), bool(Bool), object([String: JSONValue]), array([JSONValue]), null
    init(from decoder: Decoder) throws { let c = try decoder.singleValueContainer(); if c.decodeNil() { self = .null } else if let v = try? c.decode(String.self) { self = .string(v) } else if let v = try? c.decode(Bool.self) { self = .bool(v) } else if let v = try? c.decode(Double.self) { self = .number(v) } else if let v = try? c.decode([String: JSONValue].self) { self = .object(v) } else { self = .array(try c.decode([JSONValue].self)) } }
    func encode(to encoder: Encoder) throws { var c = encoder.singleValueContainer(); switch self { case .string(let v): try c.encode(v); case .number(let v): try c.encode(v); case .bool(let v): try c.encode(v); case .object(let v): try c.encode(v); case .array(let v): try c.encode(v); case .null: try c.encodeNil() } }
    var string: String? { if case .string(let v) = self { v } else { nil } }
    var bool: Bool? { if case .bool(let v) = self { v } else { nil } }
    var object: [String: JSONValue]? { if case .object(let v) = self { v } else { nil } }
}

final class HostClient: @unchecked Sendable {
    private final class Pending { let semaphore = DispatchSemaphore(value: 0); var result: Result<HostResponse, Error>? }
    private let process = Process(); private let input = Pipe(); private let output = Pipe(); private var nextID = 1; private var pending: [Int: Pending] = [:]; private let lock = NSLock()
    init(hostPath: URL, databasePath: URL) throws {
        guard FileManager.default.isExecutableFile(atPath: hostPath.path) else { throw HostClientError.executableMissing }
        process.executableURL = hostPath; process.arguments = ["--db", databasePath.path]; process.standardInput = input; process.standardOutput = output; process.standardError = FileHandle.standardError
        output.fileHandleForReading.readabilityHandler = { [weak self] handle in self?.read(handle.availableData) }
        do { try process.run() } catch { throw HostClientError.launchFailed }
    }
    deinit { if process.isRunning { process.terminate() } }
    func send(_ command: String, params: [String: JSONValue] = [:]) throws -> HostResponse {
        lock.lock(); let id = nextID; nextID += 1; let waiter = Pending(); pending[id] = waiter; lock.unlock()
        let request: [String: JSONValue] = ["protocol_version": .number(1), "id": .number(Double(id)), "command": .string(command), "params": .object(params)]
        let data = try JSONEncoder().encode(JSONValue.object(request)) + Data([10])
        input.fileHandleForWriting.write(data); waiter.semaphore.wait()
        return try waiter.result!.get()
    }
    private func read(_ data: Data) { guard let text = String(data: data, encoding: .utf8) else { return }; for line in text.split(separator: "\n") { guard let response = try? JSONDecoder().decode(HostResponse.self, from: Data(line.utf8)), let id = response.id.number.map(Int.init) else { continue }; lock.lock(); let waiter = pending.removeValue(forKey: id); lock.unlock(); guard let waiter else { continue }; waiter.result = response.ok ? .success(response) : .failure(HostClientError.operation(response.error?.message ?? "host operation failed")); waiter.semaphore.signal() } }
}

extension JSONValue { var number: Double? { if case .number(let v) = self { v } else { nil } } }

enum MenuBarVisualState: Equatable {
    case off, monitoring, attention
    static func map(status: [String: JSONValue], hostUnavailable: Bool) -> Self {
        let attention = status["attention_required"]?.bool == true || status["record_folder"] == .null || (status["awaiting_consent"]?.number ?? 0) > 0 || status["manual_update"]?.object?["last_error"]?.string != nil || status["scheduled_reconciliation"]?.object?["last_error"]?.string != nil
        if hostUnavailable || attention { return .attention }
        return status["monitoring"]?.string == "Enabled" ? .monitoring : .off
    }
    var accessibilityLabel: String { switch self { case .off: "Schomburg monitoring off"; case .monitoring: "Schomburg monitoring"; case .attention: "Schomburg needs attention" } }
    var symbol: String { self == .attention ? "circle.fill" : "circle" }
}

@MainActor final class AppModel: ObservableObject {
    @Published var status: [String: JSONValue] = [:]; @Published var error: String?; @Published var updating = false; @Published var showingSettings = false
    private var host: HostClient?
    func start() { do { let env = ProcessInfo.processInfo.environment; let hostPath = URL(fileURLWithPath: env["SCHOMBURG_HOST_PATH"] ?? FileManager.default.currentDirectoryPath + "/target/debug/schomburg-host"); let db = URL(fileURLWithPath: env["SCHOMBURG_DB_PATH"] ?? NSHomeDirectory() + "/.schomburg/machine.sqlite3"); host = try HostClient(hostPath: hostPath, databasePath: db); refresh() } catch { self.error = error.localizedDescription } }
    func refresh() { do { guard let response = try host?.send("status"), let value = response.result?.object else { return }; status = value; error = nil } catch { self.error = error.localizedDescription } }
    func updateRecord() { updating = true; defer { updating = false }; do { _ = try host?.send("update_record"); refresh() } catch { self.error = error.localizedDescription } }
    func monitoring(_ enabled: Bool) { do { _ = try host?.send("set_monitoring", params: ["enabled": .bool(enabled)]); refresh() } catch { self.error = error.localizedDescription } }
    func openToday() { guard let path = status["today_record_path"]?.string else { showingSettings = true; return }; if FileManager.default.fileExists(atPath: path) { NSWorkspace.shared.open(URL(fileURLWithPath: path)) } else { updateRecord() } }
    func openFolder() { guard let path = status["record_folder"]?.string else { showingSettings = true; return }; NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
}

@main struct SchomburgMacOSApp: App {
    @StateObject private var model = AppModel()
    @State private var pulse = false
    private var visualState: MenuBarVisualState { MenuBarVisualState.map(status: model.status, hostUnavailable: model.error != nil) }
    var body: some Scene {
        MenuBarExtra { MenuContent().environmentObject(model).onAppear { model.start() } } label: {
            Image(systemName: visualState == .monitoring && pulse ? "circle.fill" : visualState.symbol)
                .foregroundStyle(visualState == .attention ? Color.yellow : Color.primary)
                .accessibilityLabel(visualState.accessibilityLabel)
                .onAppear { if visualState == .monitoring { withAnimation(.easeInOut(duration: 1.4).repeatForever(autoreverses: true)) { pulse.toggle() } } }
        }
            .menuBarExtraStyle(.window)
        Settings { SettingsView().environmentObject(model) }
    }
}
struct MenuContent: View { @EnvironmentObject var model: AppModel
    var body: some View { VStack(alignment: .leading) { Text("Schomburg").font(.headline); Button(model.status["monitoring"]?.string == "Enabled" ? "Turn Monitoring Off" : "Turn Monitoring On") { model.monitoring(model.status["monitoring"]?.string != "Enabled") }; Button(model.updating ? "Updating…" : "Update Record", action: model.updateRecord).disabled(model.updating); Button("Today’s Record", action: model.openToday); Button("Open Record Folder", action: model.openFolder); Divider(); Button("Settings…") { model.showingSettings = true }; Button("Quit") { NSApplication.shared.terminate(nil) } }.padding() }
}
struct SettingsView: View { @EnvironmentObject var model: AppModel
    var body: some View { VStack(alignment: .leading, spacing: 12) { Text("General").font(.headline); Button("Update Record", action: model.updateRecord); Text("Record").font(.headline); Text(model.status["record_folder"]?.string ?? "Choose a Record Folder to continue."); Button("Open Folder", action: model.openFolder); Button("Today’s Record", action: model.openToday); Text("Automatic Reconciliation").font(.headline); Text("Next: \(model.status["next_scheduled_run"]?.string ?? "none")"); Text("Connections").font(.headline); Text("Connected: \(model.status["connected_sources"]?.string ?? "0")"); Text("Diagnostics").font(.headline); Text("Host protocol: 1") }.padding().frame(width: 480) }
}
