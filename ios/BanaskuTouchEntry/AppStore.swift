import Foundation
import SwiftUI

@MainActor
final class AppStore: ObservableObject {
    @Published var mode: ServiceMode = .mock {
        didSet {
            guard !isRestoringSnapshot, mode != oldValue else { return }
            guard !isBusy && !isExporting else {
                isRestoringSnapshot = true
                mode = oldValue
                isRestoringSnapshot = false
                alertMessage = "処理中またはエクスポート中はモードを変更できません。"
                return
            }
            clearWorkflowState()
            bearerToken = ""
            simulateRegistrationFailure = false
            mock.restoreState(credentials: [], reservations: [], registrations: [])
            persistSnapshot()
        }
    }
    @Published var baseURL = "https://api.example.com" { didSet { persistSnapshot() } }
    @Published var bearerToken = ""
    @Published var simulateRegistrationFailure = false
    @Published var member = TestMember.samples[0] {
        didSet {
            guard !isRestoringSnapshot, mode == .mock, member != oldValue else { return }
            guard !isBusy && !isExporting else {
                isRestoringSnapshot = true
                member = oldValue
                isRestoringSnapshot = false
                alertMessage = "処理中またはエクスポート中はテスト会員を変更できません。"
                return
            }
            clearWorkflowState()
            mock.restoreState(credentials: [], reservations: [], registrations: [])
            persistSnapshot()
        }
    }
    @Published var gateID = "gate-test-01" { didSet { persistSnapshot() } }
    @Published var startsAt = Calendar.current.date(byAdding: .minute, value: 15, to: .now) ?? .now { didSet { persistSnapshot() } }
    @Published var endsAt = Calendar.current.date(byAdding: .hour, value: 2, to: .now) ?? .now { didSet { persistSnapshot() } }
    @Published private(set) var credential: CredentialRecord? { didSet { persistSnapshot() } }
    @Published private(set) var credentialState: CredentialState = .notIssued { didSet { persistSnapshot() } }
    @Published private(set) var reservation: TestReservation? { didSet { persistSnapshot() } }
    @Published private(set) var registration: RegistrationRecord? { didSet { persistSnapshot() } }
    @Published private(set) var lastAuthorization: AuthorizationRecord? { didSet { persistSnapshot() } }
    @Published private(set) var events: [AuditEvent] = [] { didSet { persistSnapshot() } }
    @Published var isBusy = false
    @Published var isExporting = false
    @Published var alertMessage: String?

    let sessionID: String
    private let mock = MockBackend()
    private static let snapshotKey = "banasku-touch-entry.snapshot.v1"
    static let displayTimeZone = TimeZone(identifier: "Asia/Tokyo") ?? .current
    private var isRestoringSnapshot = false

    init() {
        let snapshot = Self.loadSnapshot()
        sessionID = snapshot?.sessionID ?? UUID().uuidString
        if let snapshot {
            isRestoringSnapshot = true
            mode = ServiceMode(rawValue: snapshot.mode) ?? .mock
            baseURL = snapshot.baseURL
            member = TestMember.samples.first(where: { $0.id == snapshot.memberID }) ?? TestMember.samples[0]
            gateID = snapshot.gateID
            startsAt = snapshot.startsAt
            endsAt = snapshot.endsAt
            credential = snapshot.credential
            credentialState = CredentialState(rawValue: snapshot.credentialState).flatMap { $0 == .issuing ? nil : $0 } ?? .notIssued
            reservation = snapshot.reservation
            registration = snapshot.registration
            lastAuthorization = snapshot.lastAuthorization
            events = Array(snapshot.events.suffix(200))
            isRestoringSnapshot = false
        }
        mock.restoreState(
            credentials: credential.map { [$0] } ?? [],
            reservations: reservation.map { [$0] } ?? [],
            registrations: registration.map { [$0] } ?? []
        )
    }

    var modeLabel: String { mode == .mock ? "MOCK MODE · 全操作シミュレーション" : "API MODE · サーバー接続" }
    var canChangeWorkflowContext: Bool { !isBusy && !isExporting }
    var canCreateRegistration: Bool { registration == nil || registration?.state == .cancelled }
    var credentialAvailability: CredentialState { mode == .mock ? mock.availability : .unavailableProvider }
    var canSimulateRegistrationFailure: Bool {
        if mode == .mock { return true }
        guard let host = URL(string: baseURL)?.host?.lowercased() else { return false }
        return ["localhost", "127.0.0.1"].contains(host)
    }

    func issueCredential() async {
        credentialState = .issuing
        guard let value = await run(action: "資格情報の発行", operation: {
            try await self.credentialProvider.issue(reservationID: self.reservation?.id)
        }) else { credentialState = .failed; return }
        credential = value
        credentialState = value.state
        lastAuthorization = nil
        record("資格情報の発行", result: "成功", detail: mode == .mock ? "モック参照を作成" : "サーバーの参照IDを取得。Apple Wallet/NFC発行ではありません")
    }

    func createReservation() async {
        guard endsAt > startsAt else { alertMessage = ServiceError.invalidTimeRange.localizedDescription; return }
        guard let value = await run(action: "テスト予約", operation: {
            try await self.registrationClient.createReservation(gateID: self.gateID, startsAt: self.startsAt, endsAt: self.endsAt)
        }) else { return }
        reservation = value
        lastAuthorization = nil
        record("テスト予約", result: "成功", detail: "有効時間 [\(Self.date(value.startsAt)), \(Self.date(value.endsAt)))")
    }

    func updateReservation() async {
        guard let reservation else { alertMessage = "変更する予約がありません。"; return }
        guard endsAt > startsAt else { alertMessage = ServiceError.invalidTimeRange.localizedDescription; return }
        guard let value = await run(action: "テスト予約の変更", operation: {
            try await self.registrationClient.updateReservation(id: reservation.id, startsAt: self.startsAt, endsAt: self.endsAt)
        }) else { return }
        self.reservation = value
        lastAuthorization = nil
        record("テスト予約の変更", result: "成功", detail: "有効時間 [\(Self.date(value.startsAt)), \(Self.date(value.endsAt)))")
    }

    func cancelReservation() async {
        guard let reservation else { alertMessage = "取消する予約がありません。"; return }
        guard await run(action: "テスト予約の取消", operation: { try await self.registrationClient.cancelReservation(id: reservation.id); return true }) != nil else { return }
        self.reservation?.status = "cancelled"
        if credential?.reservationID == reservation.id {
            credential?.state = .failed
            credentialState = .failed
        }
        if registration?.reservationID == reservation.id {
            registration?.state = .cancelled
            registration?.updatedAt = .now
        }
        lastAuthorization = nil
        record("テスト予約の取消", result: "取消済み", detail: "予約と関連するモック登録を取消。物理ゲートには接続していません")
    }

    func register() async {
        guard let credential, credential.state == .issued, let reservation, reservation.status == "active" else {
            alertMessage = "有効な資格情報と予約を用意してください。"
            return
        }
        let simulateFailure = simulateRegistrationFailure && canSimulateRegistrationFailure
        simulateRegistrationFailure = false
        guard let value = await run(action: "オンライン登録", operation: {
            try await self.registrationClient.register(credentialID: credential.id, reservationID: reservation.id, gateID: reservation.gateID, simulateFailure: simulateFailure)
        }) else { return }
        registration = value
        lastAuthorization = nil
        record("オンライン登録", result: value.state.label, detail: simulateFailure ? "失敗状態をローカル検証で再現。ゲート通信・タッチ・解錠は行っていません" : (mode == .mock ? "モック結果。ゲート通信・タッチ・解錠は行っていません" : "サーバー登録状態を取得。ゲート通信・タッチ・解錠は行っていません"))
    }

    func refreshRegistration() async {
        guard let registration else { alertMessage = "確認する登録がありません。"; return }
        guard let latest = await run(action: "登録状態の取得", operation: {
            try await self.registrationClient.registration(id: registration.id)
        }) else { return }
        self.registration = merged(latest, with: registration)
        record("登録状態の取得", result: latest.state.label, detail: "API/モック上の状態")
    }

    func retryRegistration() async {
        guard let registration, registration.state == .failed else { alertMessage = "再試行できる失敗登録がありません。"; return }
        guard let latest = await run(action: "登録の再試行", operation: {
            try await self.registrationClient.retryRegistration(id: registration.id)
        }) else { return }
        self.registration = merged(latest, with: registration)
        lastAuthorization = nil
        record("登録の再試行", result: latest.state.label, detail: "モック/API登録の再試行。ゲート通信・解錠は行っていません")
    }

    func cancelRegistration() async {
        guard let registration else { alertMessage = "取消する登録がありません。"; return }
        guard await run(action: "登録取消", operation: { try await self.registrationClient.cancelRegistration(id: registration.id); return true }) != nil else { return }
        self.registration?.state = .cancelled
        self.registration?.updatedAt = .now
        lastAuthorization = nil
        record("登録取消", result: "取消済み", detail: "API/モック登録を取消。物理的な入場状態は表しません")
    }

    func checkAuthorization(at evaluatedAt: Date? = nil) async {
        guard let registration else { alertMessage = "認可状態を確認する登録がありません。"; return }
        guard let result = await run(action: "時間帯認可の確認", operation: {
            try await self.registrationClient.checkAuthorization(registrationID: registration.id, evaluatedAt: evaluatedAt)
        }) else { return }
        lastAuthorization = result
        record("時間帯認可の確認", result: result.allowed ? "許可" : "拒否", detail: "\(result.reason) · mode=\(result.mode) · gate_applied=false · physical_unlock_confirmed=false")
    }

    func restoreServerState() async {
        guard mode == .api else { alertMessage = "APIモードに切り替えてから同期してください。"; return }
        guard !bearerToken.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            alertMessage = "同期する間だけBearer tokenを入力してください。アプリはtokenを保存しません。"
            return
        }
        if let current = credential,
           let latest = await run(action: "資格情報状態の同期", operation: { try await self.credentialProvider.credential(id: current.id) }) {
            credential = latest
            credentialState = latest.state
        }
        if let current = reservation,
           let latest = await run(action: "予約状態の同期", operation: { try await self.registrationClient.reservation(id: current.id) }) {
            reservation = latest
            startsAt = latest.startsAt
            endsAt = latest.endsAt
            gateID = latest.gateID
        }
        if let current = registration,
           let latest = await run(action: "登録状態の同期", operation: { try await self.registrationClient.registration(id: current.id) }) {
            registration = merged(latest, with: current)
        }
        record("保存済みAPI状態の同期", result: "完了", detail: "保存済みの参照状態をサーバーから再取得")
    }

    func fetchServerEvents() async {
        guard let remote = await run(action: "セッションログ取得", operation: { try await self.registrationClient.events(sessionID: self.sessionID) }) else { return }
        let existingIDs = Set(events.map(\.id))
        let additions = remote.filter { !existingIDs.contains("server-\($0.id)") }.map {
            AuditEvent(id: "server-\($0.id)", timestamp: $0.timestamp, action: "サーバーイベント", result: "取得済み", detail: "サーバーイベント詳細はセキュリティのため省略")
        }
        events = Array((events + additions).sorted {
            if $0.timestamp == $1.timestamp { return $0.id < $1.id }
            return $0.timestamp < $1.timestamp
        }.suffix(200))
        record("セッションログ取得", result: "成功", detail: "サーバーイベント \(additions.count) 件を追加")
    }

    func exportPayload() -> ExportPayload {
        let safeEvents = events.map { event in
            AuditEvent(id: event.id, timestamp: event.timestamp, action: event.action, result: event.result, detail: Self.sanitizedDetail(event.detail))
        }
        return ExportPayload(
            exportedAt: .now,
            sessionID: sessionID,
            mode: mode.rawValue,
            member: mode == .mock ? "\(member.name) (\(member.memberNumber))" : "Bearer認証会員 (会員選択はサーバー認証に影響しません)",
            credential: credential == nil ? "未発行" : "発行済み (IDは非表示)",
            reservation: reservation.map { "\($0.status == "active" ? "予約済み" : "取消済み") \(Self.date($0.startsAt))–\(Self.date($0.endsAt)) (IDは非表示)" } ?? "未予約",
            registration: registration.map { $0.state.label + " (IDは非表示)" } ?? "未登録",
            events: safeEvents,
            note: "このファイルはアプリ/APIの検証ログです。NFC送信、実ゲート通信、物理解錠の証跡ではありません。Bearer tokenと生の参照IDは含みません。"
        )
    }

    private var credentialProvider: CredentialProvider { mode == .mock ? mock : APIClient(baseURL: baseURL, bearerToken: bearerToken, sessionID: sessionID) }
    private var registrationClient: LockRegistrationClient { mode == .mock ? mock : APIClient(baseURL: baseURL, bearerToken: bearerToken, sessionID: sessionID) }

    private func clearWorkflowState() {
        credential = nil
        credentialState = .notIssued
        reservation = nil
        registration = nil
        lastAuthorization = nil
        events = []
    }

    private func run<T>(action: String, operation: () async throws -> T) async -> T? {
        isBusy = true
        defer { isBusy = false }
        do { return try await operation() }
        catch {
            let message = (error as? LocalizedError)?.errorDescription ?? "通信に失敗しました。URL、ネットワーク、API設定を確認してください。"
            record(action, result: "失敗", detail: message)
            alertMessage = message
            return nil
        }
    }

    private func record(_ action: String, result: String, detail: String) {
        events.insert(AuditEvent(id: UUID().uuidString, timestamp: .now, action: action, result: result, detail: detail), at: 0)
        if events.count > 200 { events = Array(events.prefix(200)) }
    }

    private func persistSnapshot() {
        let snapshot = AppSnapshot(
            sessionID: sessionID,
            mode: mode.rawValue,
            baseURL: baseURL,
            memberID: member.id,
            gateID: gateID,
            startsAt: startsAt,
            endsAt: endsAt,
            credential: credential,
            credentialState: credentialState.rawValue,
            reservation: reservation,
            registration: registration,
            lastAuthorization: lastAuthorization,
            events: Array(events.suffix(200))
        )
        guard let data = try? JSONEncoder().encode(snapshot) else { return }
        UserDefaults.standard.set(data, forKey: Self.snapshotKey)
    }

    private static func loadSnapshot() -> AppSnapshot? {
        guard let data = UserDefaults.standard.data(forKey: snapshotKey) else { return nil }
        return try? JSONDecoder().decode(AppSnapshot.self, from: data)
    }

    private func merged(_ latest: RegistrationRecord, with prior: RegistrationRecord) -> RegistrationRecord {
        RegistrationRecord(
            id: latest.id,
            credentialID: latest.credentialID.isEmpty ? prior.credentialID : latest.credentialID,
            reservationID: latest.reservationID.isEmpty ? prior.reservationID : latest.reservationID,
            gateID: latest.gateID.isEmpty ? prior.gateID : latest.gateID,
            state: latest.state,
            updatedAt: latest.updatedAt
        )
    }

    private static func sanitizedDetail(_ detail: String) -> String {
        if detail.localizedCaseInsensitiveContains("Bearer") || detail.localizedCaseInsensitiveContains("token") { return "詳細はセキュリティのため省略" }
        return detail
    }

    static func date(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "ja_JP")
        formatter.timeZone = displayTimeZone
        formatter.dateFormat = "M/d HH:mm"
        return formatter.string(from: date)
    }
}

private struct AppSnapshot: Codable {
    var sessionID: String
    var mode: String
    var baseURL: String
    var memberID: String
    var gateID: String
    var startsAt: Date
    var endsAt: Date
    var credential: CredentialRecord?
    var credentialState: String
    var reservation: TestReservation?
    var registration: RegistrationRecord?
    var lastAuthorization: AuthorizationRecord?
    var events: [AuditEvent]
}
