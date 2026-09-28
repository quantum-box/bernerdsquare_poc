import Foundation

@MainActor
protocol CredentialProvider {
    var availability: CredentialState { get }
    func issue(reservationID: String?) async throws -> CredentialRecord
    func credential(id: String) async throws -> CredentialRecord
}

@MainActor
protocol LockRegistrationClient {
    func createReservation(gateID: String, startsAt: Date, endsAt: Date) async throws -> TestReservation
    func reservation(id: String) async throws -> TestReservation
    func updateReservation(id: String, startsAt: Date, endsAt: Date) async throws -> TestReservation
    func cancelReservation(id: String) async throws
    func register(credentialID: String, reservationID: String, gateID: String, simulateFailure: Bool) async throws -> RegistrationRecord
    func registration(id: String) async throws -> RegistrationRecord
    func retryRegistration(id: String) async throws -> RegistrationRecord
    func cancelRegistration(id: String) async throws
    func checkAuthorization(registrationID: String, evaluatedAt: Date?) async throws -> AuthorizationRecord
    func events(sessionID: String) async throws -> [AuditEvent]
}

enum ServiceError: LocalizedError {
    case invalidConfiguration
    case invalidTimeRange
    case httpStatus(Int)
    case invalidResponse

    var errorDescription: String? {
        switch self {
        case .invalidConfiguration: "API URLを確認してください。HTTPS (localhostを除く) のURLが必要です。"
        case .invalidTimeRange: "終了日時は開始日時より後にしてください。"
        case .httpStatus(let code): "APIがHTTP \(code)を返しました。認証情報や設定を確認してください。"
        case .invalidResponse: "APIの応答形式を読み取れませんでした。"
        }
    }
}

@MainActor
final class MockBackend: CredentialProvider, LockRegistrationClient {
    var availability: CredentialState { .issued }
    private(set) var registrations: [String: RegistrationRecord] = [:]
    private(set) var credentials: [String: CredentialRecord] = [:]
    private(set) var reservations: [String: TestReservation] = [:]

    func issue(reservationID: String?) async throws -> CredentialRecord {
        let item = CredentialRecord(id: "mock-credential-\(UUID().uuidString.prefix(8))", state: .issued, issuedAt: .now, providerLabel: "モックプロバイダー", reservationID: reservationID)
        credentials[item.id] = item
        return item
    }

    func credential(id: String) async throws -> CredentialRecord {
        guard let item = credentials[id] else { throw ServiceError.invalidResponse }
        return item
    }

    func createReservation(gateID: String, startsAt: Date, endsAt: Date) async throws -> TestReservation {
        guard endsAt > startsAt else { throw ServiceError.invalidTimeRange }
        let item = TestReservation(id: "mock-reservation-\(UUID().uuidString.prefix(8))", gateID: gateID, startsAt: startsAt, endsAt: endsAt)
        reservations[item.id] = item
        return item
    }

    func reservation(id: String) async throws -> TestReservation {
        guard let item = reservations[id] else { throw ServiceError.invalidResponse }
        return item
    }

    func updateReservation(id: String, startsAt: Date, endsAt: Date) async throws -> TestReservation {
        guard endsAt > startsAt else { throw ServiceError.invalidTimeRange }
        guard var item = reservations[id], item.status == "active" else { throw ServiceError.invalidResponse }
        item.startsAt = startsAt
        item.endsAt = endsAt
        reservations[id] = item
        return item
    }

    func cancelReservation(id: String) async throws {
        guard var item = reservations[id] else { throw ServiceError.invalidResponse }
        item.status = "cancelled"
        reservations[id] = item
        for key in Array(credentials.keys) where credentials[key]?.reservationID == id {
            credentials[key]?.state = .failed
        }
        for key in Array(registrations.keys) where registrations[key]?.reservationID == id {
            registrations[key]?.state = .cancelled
            registrations[key]?.updatedAt = .now
        }
    }

    func register(credentialID: String, reservationID: String, gateID: String, simulateFailure: Bool) async throws -> RegistrationRecord {
        guard let credential = credentials[credentialID], credential.state == .issued,
              credential.reservationID == nil || credential.reservationID == reservationID,
              let reservation = reservations[reservationID], reservation.status == "active", reservation.gateID == gateID else { throw ServiceError.invalidResponse }
        if registrations.values.contains(where: { $0.credentialID == credentialID && $0.reservationID == reservationID && $0.state != .cancelled }) {
            throw ServiceError.httpStatus(409)
        }
        let item = RegistrationRecord(id: "mock-registration-\(UUID().uuidString.prefix(8))", credentialID: credentialID, reservationID: reservationID, gateID: gateID, state: simulateFailure ? .failed : .registered, updatedAt: .now)
        registrations[item.id] = item
        return item
    }

    func registration(id: String) async throws -> RegistrationRecord {
        guard let item = registrations[id] else { throw ServiceError.invalidResponse }
        return item
    }

    func retryRegistration(id: String) async throws -> RegistrationRecord {
        guard var item = registrations[id], item.state == .failed,
              credentials[item.credentialID]?.state == .issued,
              reservations[item.reservationID]?.status == "active" else { throw ServiceError.invalidResponse }
        item.state = .registered
        item.updatedAt = .now
        registrations[id] = item
        return item
    }

    func cancelRegistration(id: String) async throws {
        guard var item = registrations[id] else { throw ServiceError.invalidResponse }
        item.state = .cancelled
        item.updatedAt = .now
        registrations[id] = item
    }

    func checkAuthorization(registrationID: String, evaluatedAt: Date?) async throws -> AuthorizationRecord {
        guard let registration = registrations[registrationID], let reservation = reservations[registration.reservationID] else { throw ServiceError.invalidResponse }
        let at = evaluatedAt ?? .now
        let allowed = registration.state == .registered && reservation.status == "active" && at >= reservation.startsAt && at < reservation.endsAt
        let reason: String
        if registration.state != .registered { reason = "registration_inactive" }
        else if reservation.status != "active" { reason = "reservation_cancelled" }
        else if at < reservation.startsAt { reason = "before_start" }
        else if at >= reservation.endsAt { reason = "at_or_after_end" }
        else { reason = "within_window" }
        return AuthorizationRecord(allowed: allowed, reason: reason, mode: "mock", evaluatedAt: at, gateApplied: false, physicalUnlockConfirmed: false)
    }

    func events(sessionID: String) async throws -> [AuditEvent] { [] }

    func restoreState(credentials: [CredentialRecord], reservations: [TestReservation], registrations: [RegistrationRecord]) {
        self.credentials = Dictionary(uniqueKeysWithValues: credentials.map { ($0.id, $0) })
        self.reservations = Dictionary(uniqueKeysWithValues: reservations.map { ($0.id, $0) })
        self.registrations = Dictionary(uniqueKeysWithValues: registrations.map { ($0.id, $0) })
    }
}

private struct APIIdentityResponse: Decodable {
    var identity: String
}

/// One isolated transport for the configurable server contract. It never logs request headers or bodies.
@MainActor
final class APIClient: CredentialProvider, LockRegistrationClient {
    private let baseURLString: String
    private let token: String
    private let sessionID: String
    private let session: URLSession

    init(baseURL: String, bearerToken: String, sessionID: String) {
        self.baseURLString = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
        self.token = bearerToken.trimmingCharacters(in: .whitespacesAndNewlines)
        self.sessionID = sessionID
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 20
        self.session = URLSession(configuration: configuration)
    }

    var availability: CredentialState { .issued }

    func identity() async throws -> String {
        let response: APIIdentityResponse = try await request("GET", path: "/v1/identity")
        guard !response.identity.isEmpty else { throw ServiceError.invalidResponse }
        return response.identity
    }

    func issue(reservationID: String?) async throws -> CredentialRecord {
        var body: [String: String] = ["request_id": UUID().uuidString, "session_id": sessionID]
        if let reservationID { body["reservation_id"] = reservationID }
        let response: APIIssueResponse = try await request("POST", path: "/v1/credentials/issue", body: body)
        return CredentialRecord(id: response.id, state: .issued, issuedAt: .now, providerLabel: "API発行参照 (Apple発行ではありません)", reservationID: reservationID)
    }

    func credential(id: String) async throws -> CredentialRecord {
        let response: APIIssueResponse = try await request("GET", path: "/v1/credentials/\(Self.path(id))")
        return CredentialRecord(id: response.id, state: Self.credentialState(response.status), issuedAt: .now, providerLabel: "API参照 (Apple発行ではありません)", reservationID: response.reservation_id)
    }

    func createReservation(gateID: String, startsAt: Date, endsAt: Date) async throws -> TestReservation {
        guard endsAt > startsAt else { throw ServiceError.invalidTimeRange }
        let body: [String: Any] = ["request_id": UUID().uuidString, "session_id": sessionID, "gate_id": gateID, "starts_at": Self.isoString(startsAt), "ends_at": Self.isoString(endsAt)]
        let response: APIReservationResponse = try await request("POST", path: "/v1/reservations", body: body)
        return TestReservation(id: response.id, gateID: response.gate_id ?? gateID, startsAt: startsAt, endsAt: endsAt, status: response.status ?? "active")
    }

    func reservation(id: String) async throws -> TestReservation {
        let response: APIReservationResponse = try await request("GET", path: "/v1/reservations/\(Self.path(id))")
        guard let startText = response.starts_at, let endText = response.ends_at,
              let start = Self.parseDate(startText), let end = Self.parseDate(endText),
              let gateID = response.gate_id else { throw ServiceError.invalidResponse }
        return TestReservation(id: response.id, gateID: gateID, startsAt: start, endsAt: end, status: response.status ?? "active")
    }

    func updateReservation(id: String, startsAt: Date, endsAt: Date) async throws -> TestReservation {
        guard endsAt > startsAt else { throw ServiceError.invalidTimeRange }
        let body: [String: String] = ["request_id": UUID().uuidString, "session_id": sessionID, "starts_at": Self.isoString(startsAt), "ends_at": Self.isoString(endsAt)]
        let response: APIReservationResponse = try await request("PATCH", path: "/v1/reservations/\(Self.path(id))", body: body)
        return TestReservation(id: response.id, gateID: response.gate_id ?? "", startsAt: startsAt, endsAt: endsAt, status: response.status ?? "active")
    }

    func cancelReservation(id: String) async throws {
        let _: EmptyResponse = try await request("DELETE", path: "/v1/reservations/\(Self.path(id))", idempotencyKey: UUID().uuidString)
    }

    func register(credentialID: String, reservationID: String, gateID: String, simulateFailure: Bool) async throws -> RegistrationRecord {
        var body: [String: Any] = ["request_id": UUID().uuidString, "session_id": sessionID, "credential_id": credentialID, "reservation_id": reservationID, "gate_id": gateID]
        if simulateFailure { body["simulate_failure"] = true }
        let response: APIRegistrationResponse = try await request("POST", path: "/v1/registrations", body: body)
        return RegistrationRecord(id: response.id, credentialID: credentialID, reservationID: reservationID, gateID: gateID, state: Self.registrationState(response.status), updatedAt: .now)
    }

    func registration(id: String) async throws -> RegistrationRecord {
        let response: APIRegistrationResponse = try await request("GET", path: "/v1/registrations/\(Self.path(id))")
        return RegistrationRecord(id: response.id, credentialID: response.credential_id ?? "", reservationID: response.reservation_id ?? "", gateID: response.gate_id ?? "", state: Self.registrationState(response.status), updatedAt: response.updated_at ?? .now)
    }

    func retryRegistration(id: String) async throws -> RegistrationRecord {
        let body = ["request_id": UUID().uuidString, "session_id": sessionID]
        let response: APIRegistrationResponse = try await request("POST", path: "/v1/registrations/\(Self.path(id))/retry", body: body)
        return RegistrationRecord(id: response.id, credentialID: response.credential_id ?? "", reservationID: response.reservation_id ?? "", gateID: response.gate_id ?? "", state: Self.registrationState(response.status), updatedAt: response.updated_at ?? .now)
    }

    func cancelRegistration(id: String) async throws {
        let requestID = UUID().uuidString
        let _: EmptyResponse = try await request("DELETE", path: "/v1/registrations/\(Self.path(id))", idempotencyKey: requestID)
    }

    func checkAuthorization(registrationID: String, evaluatedAt: Date?) async throws -> AuthorizationRecord {
        var body: [String: Any] = ["session_id": sessionID, "registration_id": registrationID]
        if let evaluatedAt { body["evaluated_at"] = Self.isoString(evaluatedAt) }
        let response: APIAuthorizationResponse = try await request("POST", path: "/v1/authorizations/check", body: body)
        return AuthorizationRecord(allowed: response.allowed, reason: response.reason, mode: response.mode, evaluatedAt: response.evaluated_at, gateApplied: response.gate_applied, physicalUnlockConfirmed: response.physical_unlock_confirmed)
    }

    func events(sessionID: String) async throws -> [AuditEvent] {
        let encoded = sessionID.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? sessionID
        let envelope: APIEventEnvelope = try await request("GET", path: "/v1/events?session_id=\(encoded)")
        return envelope.events.map {
            AuditEvent(id: $0.id ?? UUID().uuidString, timestamp: $0.created_at ?? .now, action: $0.action ?? "APIイベント", result: $0.result ?? "記録", detail: $0.detail ?? "")
        }
    }

    private func request<T: Decodable>(_ method: String, path: String, body: Any? = nil, idempotencyKey: String? = nil) async throws -> T {
        guard let base = URL(string: baseURLString), let scheme = base.scheme?.lowercased(),
              (scheme == "https" || (scheme == "http" && ["localhost", "127.0.0.1"].contains(base.host ?? ""))),
              var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else { throw ServiceError.invalidConfiguration }
        let route = path.split(separator: "?", maxSplits: 1).first.map(String.init) ?? ""
        let prefix = components.percentEncodedPath.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let routePath = route.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath = "/" + [prefix, routePath].filter { !$0.isEmpty }.joined(separator: "/")
        if let query = path.split(separator: "?", maxSplits: 1).dropFirst().first {
            components.percentEncodedQuery = String(query)
        }
        guard let url = components.url else { throw ServiceError.invalidConfiguration }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue(sessionID, forHTTPHeaderField: "X-Session-ID")
        if !token.isEmpty { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        if let idempotencyKey { request.setValue(idempotencyKey, forHTTPHeaderField: "Idempotency-Key") }
        if let body {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw ServiceError.invalidResponse }
        guard (200..<300).contains(http.statusCode) else { throw ServiceError.httpStatus(http.statusCode) }
        if T.self == EmptyResponse.self { return EmptyResponse() as! T }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let value = try decoder.singleValueContainer().decode(String.self)
            guard let date = Self.parseDate(value) else { throw ServiceError.invalidResponse }
            return date
        }
        do { return try decoder.decode(T.self, from: data) }
        catch { throw ServiceError.invalidResponse }
    }

    private static func registrationState(_ status: String?) -> RegistrationState {
        switch status?.lowercased() {
        case "registered", "active", "ready": .registered
        case "pending", "processing", "registering": .pending
        case "cancelled", "canceled", "revoked": .cancelled
        case "expired": .expired
        case "failed", "error": .failed
        default: .pending
        }
    }

    private static func credentialState(_ status: String?) -> CredentialState {
        switch status?.lowercased() {
        case "issued": .issued
        case "revoked", "expired": .failed
        case "failed": .failed
        default: .unavailableProvider
        }
    }

    private static func path(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(CharacterSet(charactersIn: "/?#"))) ?? value
    }

    private static func isoString(_ date: Date) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: date)
    }

    nonisolated private static func parseDate(_ value: String) -> Date? {
        let precise = ISO8601DateFormatter()
        precise.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return precise.date(from: value) ?? ISO8601DateFormatter().date(from: value)
    }
}

private struct EmptyResponse: Decodable {}
