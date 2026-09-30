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

## Focused credential issuance tests

Run the issuance-only unit tests on macOS:

```sh
swift test --package-path ios --filter CredentialIssuanceTests
```

These cover mock issuance, active and cancelled reservation binding, issuance guards and provider failures, plus the API issue contract and authenticated binary Wallet-pass download.

## Apple Wallet pass-add test

The credential screen can fetch a signed `.pkpass` from the configured API, then present Apple's Wallet add confirmation through PassKit. It also retains the file-import path. This tests a signed standard Wallet pass; it does not test NFC, Secure Element, gate presentation, or physical entry.

To run the add-flow test:

1. Register a Pass Type ID in the Apple Developer account and create its Apple-issued Pass Type ID certificate. The certificate, pass type identifier, team identifier, and private key must match.
2. Set `WALLET_PASS_TYPE_IDENTIFIER`, `WALLET_TEAM_IDENTIFIER`, and `WALLET_ORGANIZATION_NAME` on the API Cloud App. Store the base64 DER PKCS#8 private key, Pass Type ID certificate, and WWDR intermediate as the three `WALLET_*` secrets documented in [`docs/api.md`](../docs/api.md). Keep the private key in the Cloud App secret store; do not put it in the app, D1, Git, or the manifest.
3. In API mode, authenticate, issue a test credential, then tap **Apple Walletへ追加**. The API returns a signed test membership pass with a static QR that explicitly has no gate or NFC integration.
4. On iPhone, PassKit presents the Wallet confirmation. If the Simulator cannot present PassKit's add sheet, the app exports the `.pkpass`; drag that file onto the iOS Simulator to add it to the simulated Wallet, as described by [Apple's pass-building guide](https://developer.apple.com/documentation/walletpasses/building-a-pass).

The Pass Type ID is registered and its Apple production certificate was downloaded locally on 2026-09-29; it matches the local PKCS#8 key. The certificate and key are not stored in this repository. A locally generated `.pkpass` passed CMS signature verification and was added to the iOS Simulator, where its test member, card ID, and QR code were visible. CloudApp signing secrets are not configured yet, so issuance through the API and the app's API-to-PassKit add flow remain unverified. App Store Connect is not required for a development add-flow test. NFC/SE issuance still needs separate Apple approval and compatible gate hardware.

To install the app on a physical iPhone, choose your Apple Development team under **Signing & Capabilities** in Xcode, set a unique bundle identifier if needed, connect a supported iPhone, trust the development profile, and run the scheme.

## App Store Connect and distribution

App Store Connect is not needed to build or run this prototype in the Simulator. For a personal on-device development install, Xcode can sign with an Apple Account's Personal Team; Apple limits those profiles and they expire after seven days. For TestFlight or App Store distribution, the organization needs an active Apple Developer Program membership and an App Store Connect app record before uploading a build. Register an explicit App ID whose bundle ID matches the Xcode target, and give the person uploading the build an appropriate App Store Connect role. The Account Holder must accept the current agreements before creating the app record.

### TestFlight auto upload

The [`testflight.yml`](../.github/workflows/testflight.yml) workflow archives the iOS app and uploads it to App Store Connect when iOS files change on `main`. It can also be started manually from the Actions tab on `main`. The upload goes to TestFlight after Apple's processing; it does not submit the app for App Store review or add testers to a testing group. The build number comes from the GitHub Actions run number and attempt.

The Apple and GitHub configuration was started on 2026-09-30:

- Apple Developer App ID `jp.quantumbox.banasku.touch-entry-poc` is registered with the Wallet capability enabled.
- The App Store Connect app record **Banasku Touch Entry** is registered for iOS with Japanese as its primary language and SKU `banasku-touch-entry-poc`.
- A Team API Key with the **Developer** role was created for App Store Connect upload. Its private `.p8` key is stored outside this repository. CI uses manual signing so this key does not need cloud-managed distribution-certificate access.
- These Actions repository secrets are configured in `quantum-box/bernerdsquare_poc`:

   | Secret | Value |
   | --- | --- |
   | `APPLE_TEAM_ID` | Quantum Box Apple Developer Team ID |
   | `APP_STORE_CONNECT_API_KEY_ID` | Team API key ID |
   | `APP_STORE_CONNECT_API_ISSUER_ID` | App Store Connect issuer ID |
   | `APP_STORE_CONNECT_API_PRIVATE_KEY` | Full contents of the downloaded `.p8` file |

The private API key is written to the temporary GitHub runner only for the upload. No certificate, profile, or API key is committed to the repository.

#### Signing secrets required by CI

Before the workflow can produce an uploadable build, create a GitHub Actions environment named `testflight` and restrict its deployment branches to `main`. Then create an Apple Distribution certificate and an App Store provisioning profile for `jp.quantumbox.banasku.touch-entry-poc`. The provisioning profile must include the app's enabled Wallet capability and use the same Apple Developer team as `APPLE_TEAM_ID`. Export the certificate and its private key together as a password-protected `.p12` file, then add these environment secrets to `testflight`:

| Secret | Value |
| --- | --- |
| `IOS_DISTRIBUTION_CERTIFICATE_P12_BASE64` | Base64-encoded password-protected `.p12` file |
| `IOS_DISTRIBUTION_CERTIFICATE_PASSWORD` | Password used to export the `.p12` file |
| `IOS_APP_STORE_PROVISIONING_PROFILE_BASE64` | Base64-encoded App Store provisioning profile (`.mobileprovision`) |

The workflow validates the profile's team, bundle ID, App Store distribution type, and Wallet entitlement. It imports the certificate into a temporary runner keychain, signs and uploads the archive, then removes the key, certificate, profile, and keychain. For example, encode each binary file on macOS with `base64 -i file | tr -d '\n'`; keep the `.p12` private key out of Git and chat. After the environment and its three secrets are configured and the workflow change reaches `main`, an iOS change or a manual run on `main` starts the upload. Apple processes the build before it appears in TestFlight.

The App ID's Wallet capability supports the app's standard Wallet pass entitlement. It does not grant NFC & SE or other restricted entitlements. Apple entitlement approval and the gate provider's compatibility confirmation remain separate requirements for real credential issuance.

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
