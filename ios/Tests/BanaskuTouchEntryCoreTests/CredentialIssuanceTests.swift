import Foundation
import XCTest
@testable import BanaskuTouchEntryCore

final class CredentialIssuanceTests: XCTestCase {
    @MainActor
    func testAppStoreIssuesMockReferenceAndBindsOnlyActiveReservation() async {
        let fixture = makeStore()
        defer { fixture.defaults.removePersistentDomain(forName: fixture.suiteName) }
        fixture.store.startsAt = .now.addingTimeInterval(-60)
        fixture.store.endsAt = .now.addingTimeInterval(3_600)

        await fixture.store.createReservation()
        let reservationID = fixture.store.reservation?.id
        await fixture.store.issueCredential()

        XCTAssertEqual(fixture.provider.issueCalls, 1)
        XCTAssertEqual(fixture.store.credential?.state, .issued)
        XCTAssertEqual(fixture.store.credential?.reservationID, reservationID)
        XCTAssertTrue(fixture.store.credential?.id.hasPrefix("mock-credential-") == true)
        XCTAssertTrue(fixture.store.credential?.providerLabel.contains("モック") == true)
        XCTAssertEqual(fixture.store.events.first?.action, "資格情報の発行")
        XCTAssertEqual(fixture.store.events.first?.result, "成功")
    }

    @MainActor
    func testAppStoreDoesNotBindCancelledReservation() async {
        let fixture = makeStore()
        defer { fixture.defaults.removePersistentDomain(forName: fixture.suiteName) }
        await fixture.store.createReservation()
        await fixture.store.cancelReservation()

        await fixture.store.issueCredential()

        XCTAssertEqual(fixture.provider.issueCalls, 1)
        XCTAssertNil(fixture.store.credential?.reservationID)
        XCTAssertEqual(fixture.store.credential?.state, .issued)
    }

    @MainActor
    func testAppStoreRejectsIssuanceWithoutAppliedAPIToken() async {
        let fixture = makeStore()
        defer { fixture.defaults.removePersistentDomain(forName: fixture.suiteName) }
        fixture.store.mode = .api
        fixture.store.bearerTokenDraft = ""

        await fixture.store.issueCredential()

        XCTAssertEqual(fixture.provider.issueCalls, 0)
        XCTAssertEqual(fixture.store.credentialState, .notIssued)
        XCTAssertTrue(fixture.store.alertMessage?.contains("Bearer token") == true)
    }

    @MainActor
    func testAppStoreRejectsAnotherIssueWhileRegistrationIsActiveThenAllowsAfterCancellation() async {
        let fixture = makeStore()
        defer { fixture.defaults.removePersistentDomain(forName: fixture.suiteName) }
        fixture.store.startsAt = .now.addingTimeInterval(-60)
        fixture.store.endsAt = .now.addingTimeInterval(3_600)
        await fixture.store.createReservation()
        await fixture.store.issueCredential()
        let firstCredentialID = fixture.store.credential?.id
        await fixture.store.register()
        XCTAssertEqual(fixture.store.registration?.state, .registered)

        await fixture.store.issueCredential()

        XCTAssertEqual(fixture.provider.issueCalls, 1)
        XCTAssertEqual(fixture.store.credential?.id, firstCredentialID)
        XCTAssertTrue(fixture.store.alertMessage?.contains("登録を取消") == true)

        await fixture.store.cancelRegistration()
        await fixture.store.issueCredential()

        XCTAssertEqual(fixture.provider.issueCalls, 2)
        XCTAssertNotEqual(fixture.store.credential?.id, firstCredentialID)
        XCTAssertNil(fixture.store.registration)
    }

    @MainActor
    func testAppStoreReportsProviderFailureWithoutCreatingIssuedRecord() async {
        let fixture = makeStore()
        defer { fixture.defaults.removePersistentDomain(forName: fixture.suiteName) }
        fixture.provider.shouldFailIssue = true

        await fixture.store.issueCredential()

        XCTAssertEqual(fixture.provider.issueCalls, 1)
        XCTAssertNil(fixture.store.credential)
        XCTAssertEqual(fixture.store.credentialState, .failed)
        XCTAssertEqual(fixture.store.events.first?.result, "失敗")
        XCTAssertTrue(fixture.store.alertMessage?.contains("HTTP 503") == true)
    }

    @MainActor
    func testAppStoreDoesNotCallProviderWhenBusy() async {
        let fixture = makeStore()
        defer { fixture.defaults.removePersistentDomain(forName: fixture.suiteName) }
        fixture.store.isBusy = true

        await fixture.store.issueCredential()

        XCTAssertEqual(fixture.provider.issueCalls, 0)
        XCTAssertEqual(fixture.store.credentialState, .notIssued)
        XCTAssertTrue(fixture.store.alertMessage?.contains("処理中") == true)
    }

    @MainActor
    func testAPIClientPostsAuthenticatedIssueRequestAndDecodesIssuedReference() async throws {
        StubURLProtocol.reset {
            request in
            XCTAssertEqual(request.httpMethod, "POST")
            XCTAssertEqual(request.url?.path, "/api/v1/credentials/issue")
            let response = HTTPURLResponse(
                url: try XCTUnwrap(request.url),
                statusCode: 201,
                httpVersion: nil,
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, Data(#"{"id":"credential-123","status":"issued","reservation_id":"reservation-456","wallet_pass_url":"/v1/credentials/credential-123/pass"}"#.utf8))
        }
        let client = makeAPIClient()

        let record = try await client.issue(reservationID: "reservation-456")

        XCTAssertEqual(record.id, "credential-123")
        XCTAssertEqual(record.state, .issued)
        XCTAssertEqual(record.reservationID, "reservation-456")
        XCTAssertEqual(record.walletPassURL, "/v1/credentials/credential-123/pass")
        XCTAssertTrue(record.providerLabel.contains("署名済みWalletテストパス"))
        let request = try XCTUnwrap(StubURLProtocol.lastRequest)
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
        XCTAssertEqual(request.value(forHTTPHeaderField: "X-Session-ID"), "session-test")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Content-Type"), "application/json")
        let body = try XCTUnwrap(try JSONSerialization.jsonObject(with: XCTUnwrap(StubURLProtocol.lastRequestBody)) as? [String: String])
        XCTAssertEqual(body["session_id"], "session-test")
        XCTAssertEqual(body["reservation_id"], "reservation-456")
        XCTAssertFalse(try XCTUnwrap(body["request_id"]).isEmpty)
    }

    @MainActor
    func testAPIClientDownloadsWalletPassWithBearerAndExpectedContentType() async throws {
        let passData = Data([0x50, 0x4B, 0x03, 0x04, 0x01, 0x02])
        StubURLProtocol.reset { request in
            XCTAssertEqual(request.httpMethod, "GET")
            XCTAssertEqual(request.url?.path, "/api/v1/credentials/credential-123/pass")
            XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
            XCTAssertEqual(request.value(forHTTPHeaderField: "X-Session-ID"), "session-test")
            XCTAssertEqual(request.value(forHTTPHeaderField: "Accept"), "application/vnd.apple.pkpass")
            let response = HTTPURLResponse(
                url: try XCTUnwrap(request.url),
                statusCode: 200,
                httpVersion: nil,
                headerFields: ["Content-Type": "application/vnd.apple.pkpass"]
            )!
            return (response, passData)
        }

        let received = try await makeAPIClient().walletPass(id: "credential-123")

        XCTAssertEqual(received, passData)
    }

    @MainActor
    func testAPIClientRejectsNonWalletPassResponseContentType() async {
        StubURLProtocol.reset { request in
            let response = HTTPURLResponse(
                url: try XCTUnwrap(request.url),
                statusCode: 200,
                httpVersion: nil,
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, Data(#"{"error":"unexpected"}"#.utf8))
        }

        do {
            _ = try await makeAPIClient().walletPass(id: "credential-123")
            XCTFail("Expected non-pass content type to be rejected")
        } catch {
            guard case ServiceError.invalidResponse = error else {
                XCTFail("Expected invalidResponse, got \(error)")
                return
            }
        }
    }

    @MainActor
    func testAPIClientOmitsReservationWhenNoneWasRequested() async throws {
        StubURLProtocol.reset { request in
            let response = HTTPURLResponse(
                url: try XCTUnwrap(request.url),
                statusCode: 201,
                httpVersion: nil,
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, Data(#"{"id":"credential-789","status":"issued","reservation_id":null}"#.utf8))
        }

        let record = try await makeAPIClient().issue(reservationID: nil)

        XCTAssertEqual(record.id, "credential-789")
        XCTAssertNil(record.reservationID)
        _ = try XCTUnwrap(StubURLProtocol.lastRequest)
        let body = try XCTUnwrap(try JSONSerialization.jsonObject(with: XCTUnwrap(StubURLProtocol.lastRequestBody)) as? [String: String])
        XCTAssertNil(body["reservation_id"])
    }

    @MainActor
    func testAPIClientRejectsEmptyIDWrongStatusAndReservationMismatch() async {
        let invalidPayloads = [
            #"{"id":"  ","status":"issued","reservation_id":"reservation-456"}"#,
            #"{"id":"credential-123","status":"failed","reservation_id":"reservation-456"}"#,
            #"{"id":"credential-123","status":"issued","reservation_id":"other-reservation"}"#,
            #"{"id":"credential-123","status":"issued","reservation_id":"reservation-456","wallet_pass_url":"https://unexpected.example/pass"}"#
        ]

        for payload in invalidPayloads {
            StubURLProtocol.reset { request in
                let response = HTTPURLResponse(
                    url: try XCTUnwrap(request.url),
                    statusCode: 201,
                    httpVersion: nil,
                    headerFields: ["Content-Type": "application/json"]
                )!
                return (response, Data(payload.utf8))
            }
            do {
                _ = try await makeAPIClient().issue(reservationID: "reservation-456")
                XCTFail("Invalid issue response must be rejected: \(payload)")
            } catch {
                guard case ServiceError.invalidResponse = error else {
                    XCTFail("Expected invalidResponse, got \(error)")
                    continue
                }
            }
        }
    }

    @MainActor
    func testAPIClientPropagatesUnauthorizedResponse() async {
        StubURLProtocol.reset { request in
            let response = HTTPURLResponse(
                url: try XCTUnwrap(request.url),
                statusCode: 401,
                httpVersion: nil,
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, Data(#"{"error":"unauthorized"}"#.utf8))
        }

        do {
            _ = try await makeAPIClient().issue(reservationID: nil)
            XCTFail("Unauthorized issue request must fail")
        } catch {
            guard case ServiceError.httpStatus(401) = error else {
                XCTFail("Expected HTTP 401, got \(error)")
                return
            }
        }
    }

    @MainActor
    func testAPIClientRejectsNonHTTPSIssueEndpointBeforeSendingRequest() async {
        StubURLProtocol.reset { request in
            let response = HTTPURLResponse(
                url: try XCTUnwrap(request.url),
                statusCode: 201,
                httpVersion: nil,
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, Data(#"{"id":"credential-123","status":"issued","reservation_id":null}"#.utf8))
        }
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [StubURLProtocol.self]
        let client = APIClient(
            baseURL: "http://api.example.test",
            bearerToken: "test-token",
            sessionID: "session-test",
            session: URLSession(configuration: config)
        )

        do {
            _ = try await client.issue(reservationID: nil)
            XCTFail("Insecure remote issue endpoint must fail")
        } catch {
            guard case ServiceError.invalidConfiguration = error else {
                XCTFail("Expected invalidConfiguration, got \(error)")
                return
            }
        }
        XCTAssertNil(StubURLProtocol.lastRequest)
    }

    @MainActor
    private func makeStore() -> (store: AppStore, provider: TrackingCredentialProvider, defaults: UserDefaults, suiteName: String) {
        let suiteName = "CredentialIssuanceTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        let backend = MockBackend()
        let provider = TrackingCredentialProvider(backend: backend)
        let store = AppStore(snapshotDefaults: defaults, mockBackend: backend, credentialProviderOverride: provider)
        return (store, provider, defaults, suiteName)
    }

    @MainActor
    private func makeAPIClient() -> APIClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [StubURLProtocol.self]
        return APIClient(
            baseURL: "https://api.example.test/api",
            bearerToken: " test-token ",
            sessionID: "session-test",
            session: URLSession(configuration: config)
        )
    }
}

@MainActor
private final class TrackingCredentialProvider: CredentialProvider {
    let backend: MockBackend
    private(set) var issueCalls = 0
    var shouldFailIssue = false

    init(backend: MockBackend) { self.backend = backend }

    var availability: CredentialState { backend.availability }

    func issue(reservationID: String?) async throws -> CredentialRecord {
        issueCalls += 1
        if shouldFailIssue { throw ServiceError.httpStatus(503) }
        return try await backend.issue(reservationID: reservationID)
    }

    func credential(id: String) async throws -> CredentialRecord {
        try await backend.credential(id: id)
    }

    func walletPass(id: String) async throws -> Data {
        try await backend.walletPass(id: id)
    }
}

private final class StubURLProtocol: URLProtocol {
    typealias ResponseHandler = (URLRequest) throws -> (HTTPURLResponse, Data)
    private static let lock = NSLock()
    private static var responseHandler: ResponseHandler?
    private(set) static var lastRequest: URLRequest?
    private(set) static var lastRequestBody: Data?

    static func reset(_ handler: @escaping ResponseHandler) {
        lock.lock()
        defer { lock.unlock() }
        responseHandler = handler
        lastRequest = nil
        lastRequestBody = nil
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.lock.lock()
        let handler = Self.responseHandler
        Self.lastRequest = request
        Self.lastRequestBody = Self.readBody(from: request)
        Self.lock.unlock()
        guard let handler else {
            client?.urlProtocol(self, didFailWithError: ServiceError.invalidResponse)
            return
        }
        do {
            let (response, data) = try handler(request)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    private static func readBody(from request: URLRequest) -> Data? {
        if let body = request.httpBody { return body }
        guard let stream = request.httpBodyStream else { return nil }
        stream.open()
        defer { stream.close() }
        var result = Data()
        var buffer = [UInt8](repeating: 0, count: 1_024)
        while stream.hasBytesAvailable {
            let count = stream.read(&buffer, maxLength: buffer.count)
            if count <= 0 { break }
            result.append(buffer, count: count)
        }
        return result.isEmpty ? nil : result
    }

    override func stopLoading() {}
}
