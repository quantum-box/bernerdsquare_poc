# Banasku Touch Entry Prototype

Native SwiftUI iPhone prototype for COM-876. All source and project files are scoped to this `ios/` directory.

## Open and run

1. Open `BanaskuTouchEntry.xcodeproj` in Xcode 27 or newer.
2. Select the shared `BanaskuTouchEntry` scheme and an iPhone Simulator or a signed iPhone device.
3. Run the app. The default **モック** mode supports test member selection, mock credential reference issuance, reservation create/update/cancel, online registration and retry, registration status refresh/cancel, authorization checks at reservation boundaries, session log, and sanitized JSON export.

Command line build for an installed simulator runtime:

```sh
xcodebuild -project ios/BanaskuTouchEntry.xcodeproj \
  -scheme BanaskuTouchEntry \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  CODE_SIGNING_ALLOWED=NO build
```

To install on a physical iPhone, choose your Apple Development team under **Signing & Capabilities** in Xcode, set a unique bundle identifier if needed, connect a supported iPhone, trust the development profile, and run the scheme. This prototype does not request Apple credential/NFC entitlements and cannot issue an Apple Wallet badge.

## API mode

Select **API** in Settings and provide the API base URL and bearer token. HTTPS is required, except for localhost development. The token stays in process memory and is not written to UserDefaults, the audit log, or export. The app saves test-flow state and server reference IDs locally; enter the token again after relaunch and use **保存済み状態をサーバーと同期** to refresh them.

Implemented contract:

- `POST /v1/credentials/issue` with `request_id` and optional `reservation_id`
- `GET /v1/credentials/{id}`
- `POST /v1/reservations` with `request_id`, `gate_id`, and ISO-8601 `starts_at` / `ends_at`
- `GET/PATCH/DELETE /v1/reservations/{id}`
- `POST /v1/registrations` with `request_id`, `credential_id`, `reservation_id`, and `gate_id`
- `GET /v1/registrations/{id}`, `POST /v1/registrations/{id}/retry`, `DELETE /v1/registrations/{id}`
- `POST /v1/authorizations/check`
- `GET /v1/events?session_id=...`

The app includes simulated before-start, start, just-before-end, and end checks in mock mode. API mode checks the current server time. The **次の登録を失敗させる** setting exposes the failed-registration/retry path in mock mode and for localhost API targets configured with `ALLOW_TIME_SIMULATION=true`. Every authorization result is a server/mock decision and reports that no gate action or physical unlock was confirmed.

Reservations use a half-open interval: the start is included and the end is excluded. API-returned credential IDs are server references; they do not mean a credential was provisioned to Apple Wallet or the Secure Element. The app never emulates arbitrary NFC, FeliCa, or UIDs, and never reports a tap or physical unlock.
