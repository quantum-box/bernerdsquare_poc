import Foundation

enum ServiceMode: String, CaseIterable, Identifiable {
    case mock = "モック"
    case api = "API"
    var id: String { rawValue }
}

enum CredentialState: String, Codable {
    case notIssued, issuing, issued, unavailableEntitlement, unavailableDevice, unavailableProvider, failed

    var label: String {
        switch self {
        case .notIssued: "未発行"
        case .issuing: "発行中"
        case .issued: "参照情報あり"
        case .unavailableEntitlement: "権限未設定"
        case .unavailableDevice: "非対応デバイス"
        case .unavailableProvider: "発行プロバイダー未設定"
        case .failed: "発行失敗"
        }
    }
}

enum RegistrationState: String, Codable {
    case notRegistered, registering, pending, registered, failed, cancelled, expired

    var label: String {
        switch self {
        case .notRegistered: "未登録"
        case .registering: "登録処理中"
        case .pending: "反映待ち"
        case .registered: "登録済み"
        case .failed: "登録失敗"
        case .cancelled: "取消済み"
        case .expired: "失効"
        }
    }
}

struct TestMember: Identifiable, Codable, Hashable {
    var id: String
    var name: String
    var memberNumber: String
    static let samples = [
        TestMember(id: "member-001", name: "テスト会員 A", memberNumber: "TEST-001"),
        TestMember(id: "member-002", name: "テスト会員 B", memberNumber: "TEST-002"),
        TestMember(id: "member-003", name: "テスト会員 C", memberNumber: "TEST-003")
    ]
}

struct CredentialRecord: Codable, Identifiable {
    var id: String
    var state: CredentialState
    var issuedAt: Date
    var providerLabel: String
    var reservationID: String? = nil
    var walletPassURL: String? = nil
}

struct TestReservation: Codable, Identifiable {
    var id: String
    var gateID: String
    var startsAt: Date
    var endsAt: Date
    var status: String = "active"
}

struct RegistrationRecord: Codable, Identifiable {
    var id: String
    var credentialID: String
    var reservationID: String
    var gateID: String
    var state: RegistrationState
    var updatedAt: Date
}

struct AuditEvent: Codable, Identifiable {
    var id: String
    var timestamp: Date
    var action: String
    var result: String
    var detail: String
}

struct ExportPayload: Codable {
    var exportedAt: Date
    var sessionID: String
    var mode: String
    var member: String
    var credential: String
    var reservation: String
    var registration: String
    var events: [AuditEvent]
    var note: String
}

struct APIIssueResponse: Decodable {
    var id: String
    var status: String?
    var reservation_id: String?
    var wallet_pass_url: String?
}
struct APIReservationResponse: Decodable {
    var id: String
    var gate_id: String?
    var starts_at: String?
    var ends_at: String?
    var status: String?
}
struct APIRegistrationResponse: Decodable {
    var id: String
    var status: String?
    var credential_id: String?
    var reservation_id: String?
    var gate_id: String?
    var updated_at: Date?
    var gate_applied: Bool?
    var physical_unlock_confirmed: Bool?
}
struct APIEventEnvelope: Decodable { var events: [APIEvent] }
struct APIEvent: Decodable {
    var id: String?
    var created_at: Date?
    var action: String?
    var result: String?
    var detail: String?
}

struct AuthorizationRecord: Codable {
    var allowed: Bool
    var reason: String
    var mode: String
    var evaluatedAt: Date
    var gateApplied: Bool
    var physicalUnlockConfirmed: Bool
}

struct APIAuthorizationResponse: Decodable {
    var allowed: Bool
    var reason: String
    var mode: String
    var evaluated_at: Date
    var gate_applied: Bool
    var physical_unlock_confirmed: Bool
}
