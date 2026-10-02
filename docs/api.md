# Prototype API

All API routes require `Authorization: Bearer <token>`. The Worker reads `API_BEARER_TOKENS_JSON` as a secret JSON object mapping opaque tokens to server-side member IDs. The client never submits an owner ID. Requests are scoped to that mapped owner. Invalid credentials receive `401`; a resource owned by another member is indistinguishable from a missing resource.

The credential and gate adapters remain mock implementations. The API persists state in D1 and returns `mode: "mock"`; `gate_applied` and `physical_unlock_confirmed` are always false. When an Apple Pass Type ID signer is configured, the API can also generate a standard signed generic Wallet pass for testing. That pass has a static test QR and is not a gate credential.

## Routes

| Method | Path | Purpose |
|---|---|---|
| GET | `/healthz` | Health and non-secret mock/auth configuration state |
| GET | `/v1/identity` | Return the authenticated account's stable opaque identity |
| POST | `/v1/credentials/issue` | Create a mock credential reference; no NFC payload is created |
| GET | `/v1/credentials/{id}` | Read an owner-scoped credential |
| GET | `/v1/credentials/{id}/pass` | Return the owner's signed test `.pkpass` when Wallet signing is configured |
| POST | `/v1/reservations` | Create a test reservation; interval is `[starts_at, ends_at)` |
| GET | `/v1/reservations/{id}` | Read an owner-scoped reservation |
| PATCH | `/v1/reservations/{id}` | Change the complete test reservation interval |
| DELETE | `/v1/reservations/{id}` | Cancel the reservation and revoke its mock registrations |
| POST | `/v1/registrations` | Request a mock registration for an owned credential and reservation |
| GET | `/v1/registrations/{id}` | Read registration state and simulated adapter result |
| DELETE | `/v1/registrations/{id}` | Revoke the mock registration |
| POST | `/v1/registrations/{id}/retry` | Retry a failed mock registration |
| POST | `/v1/authorizations/check` | Simulate whether an active registration is within its reservation window |
| GET | `/v1/events?session_id={id}` | Read sanitized owner-scoped audit events |

`GET /v1/identity` requires the same Bearer authentication as other `/v1` routes and returns `200 OK` with `{"identity":"<sha256-hex>"}`. The value is SHA-256 of the authenticated server-side `owner_id`, so it is stable for the same account across token changes and does not disclose the raw owner ID. Different accounts receive different identity values except for the negligible possibility of a SHA-256 collision.

Mutation bodies carry `request_id` for idempotency and may carry a `session_id` for correlation. The app also sends `X-Session-ID`; `DELETE` routes use that value for event correlation and an `Idempotency-Key` header for replay protection. A repeated key with the same request returns the stored response. Reusing it with a different body returns `409`.

## Time checks

All times must be RFC 3339 with an explicit offset. The API normalizes stored values to UTC. Authorization is allowed only when the reservation is active, the mock registration is active, and `starts_at <= now < ends_at`. Supplying `evaluated_at` is available only when `ALLOW_TIME_SIMULATION=true`, which is intended for local test runs. It never simulates a physical reader or unlock.

For a local failure/retry check, create a registration with `simulate_failure: true` while `ALLOW_TIME_SIMULATION=true`, then call `/v1/registrations/{id}/retry` with a new `request_id`. The switch is rejected when the Worker variable is false. The iOS mock screen exposes boundary checks; API mode checks current server time, because custom `evaluated_at` is accepted only by a Worker configured for local time simulation. Repeating a registration request with the same request ID is idempotent; another request ID for the same active or failed credential-reservation pair returns `409` instead of creating a duplicate.

## Local development

1. Install rustup and Wrangler. [`api/rust-toolchain.toml`](../api/rust-toolchain.toml) pins Rust 1.91.0 and the `wasm32-unknown-unknown` target; install `worker-build` under that toolchain for local builds.
2. Create `api/.dev.vars` from `.dev.vars.example` and use a fresh local-only token.
3. From `api/`, run `wrangler d1 migrations apply banasku-touch-entry-local --local --config wrangler.local.toml`, then start `wrangler dev --config wrangler.local.toml`.
4. The repository-root Tachyon manifest targets the `bernard-square` CloudApp, provisions its D1 binding, and requests `build.runnerBackend: kubernetes_kata` with `deploymentTarget: cloudflare_workers`. Tachyon's Rust Worker bootstrap installs `worker-build` before custom install commands, so the API pins Rust 1.91.0 in its toolchain file; the current `worker-build` 0.8.7 requires at least that version, while the Runner default is 1.88. No successful Cloud App build or deployment has been verified yet. Keep `API_BEARER_TOKENS_JSON` in the sandbox/preview Cloud App secret store and outside Git.

The deployed CloudApp uses the same mock adapter as local mode. It never contacts a gate or confirms a physical unlock.

## Apple Wallet pass status

`POST /v1/credentials/issue` creates an owner-scoped test reference and includes `wallet_pass_url` only when all Wallet signing configuration is present. Fetch that path with the same Bearer token to receive `application/vnd.apple.pkpass`. The endpoint checks that the credential belongs to the caller and remains issued, and sends `Cache-Control: no-store`.

Before advertising a Wallet URL, the Worker checks that the PKCS#8 key matches the Pass Type ID certificate, the certificate subject matches the configured Pass Type ID and Team ID, the signer certificate allows digital signatures and carries Apple's Pass Type ID extended key usage, both certificates are current, and the signer certificate verifies against the pinned Apple WWDR G4 certificate. The Worker does not query Apple's certificate revocation list; configure only an active, unrevoked Apple certificate. The detached CMS structure and RSA signature are covered by focused tests using test-only keys and certificates.

Configure the non-secret `WALLET_PASS_TYPE_IDENTIFIER`, `WALLET_TEAM_IDENTIFIER`, and `WALLET_ORGANIZATION_NAME` Worker variables in `tachyon.yml`. Store the base64-encoded DER PKCS#8 private key, Pass Type ID certificate, and Apple WWDR intermediate only as Cloud App secrets named `WALLET_SIGNER_PRIVATE_KEY_PKCS8_B64`, `WALLET_SIGNER_CERTIFICATE_DER_B64`, and `WALLET_WWDR_CERTIFICATE_DER_B64`. Never put the private key in the iOS app, D1, Git, build logs, or `tachyon.yml`; configure the signing secrets after the Apple-issued Pass Type ID certificate and WWDR intermediate are available.

The Pass Type ID was already registered in the Quantum Box, Inc. Apple Developer team. Its Apple production certificate was downloaded on 2026-09-29, is valid through 2027-10-29, and matches the local PKCS#8 private key. A locally generated `.pkpass` passed CMS signature verification and was added to the iOS Simulator, where its test member, card ID, and QR code were visible. This verifies local signing and the Simulator Wallet add flow; it does not verify the Rust Worker endpoint, CloudApp secrets, NFC presentation, or gate compatibility.

The iOS API screen exposes **Apple Walletへ追加** after the API returns `wallet_pass_url`. It downloads the binary with Bearer authentication and presents PassKit's add sheet when available; if the runtime cannot present that sheet, it exports the `.pkpass` for opening or dragging into iOS Simulator. A standard QR/barcode pass does not implement NFC or prove gate compatibility. App Store Connect is not needed to issue or add a development pass; the Apple Developer Pass Type ID and its Apple-issued certificate are needed to sign one.
