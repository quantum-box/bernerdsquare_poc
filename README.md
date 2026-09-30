# Banasku Touch Entry Prototype

Native SwiftUI iPhone app and a Rust API built for Cloudflare Workers. This repository starts in **mock mode**: credentials and lock registrations are simulated, no NFC credential is emitted, and no physical gate operation is reported as successful.

## Components

- `ios/` — iPhone test app with mock and API-backed flows; [TestFlight auto-upload setup](ios/README.md#testflight-auto-upload).
- `api/` — Rust/axum Worker API, D1 migrations, and local Wrangler configuration.
- `docs/credential-technology.md` — Apple credential options and current eligibility findings.
- `docs/device-compatibility.md` — equipment details still needed to decide gate compatibility.
- `docs/api.md` — API contract and local setup.

## Safety boundary

The API has only mock credential and lock adapters. `gate_applied` and `physical_unlock_confirmed` remain false, including when the mock registration state is `registered`. Production gate credentials, Apple signing keys, and lock-management secrets are not stored in the app or repository. The Wallet CMS test uses a disposable test-only key and certificates; they cannot issue a production pass. Configure test-member bearer tokens as Worker secrets; the server maps each token to an owner and never accepts an owner ID from the request body.

## Current validation boundary

The code and local run instructions are prepared for simulator and Worker development. Apple entitlements, Apple Business Register onboarding, real gate hardware, vendor APIs, CloudApp bindings, and physical unlock behavior require their external owners and equipment. See the docs before treating any step as real issuance, registration, or entry.
