# iPhone credential technology findings

Reviewed 2026-09-29 against Apple Developer documentation.

## Current classification

| Route | Classification | Evidence and condition |
|---|---|---|
| Apple Wallet generic membership pass (file/QR) | Signed pass verified and added to iOS Simulator; online API issuance still unverified | The existing Pass Type ID and Apple production certificate are present in the Quantum Box team. The downloaded certificate matches the local PKCS#8 private key and is valid through 2027-10-29. A locally generated `.pkpass` passed CMS signature verification and was added to the iOS Simulator, showing the test member, card ID, and QR code. CloudApp signing secrets and the Rust Worker issuance endpoint have not been configured or exercised. This does not create contactless NFC presentation. A static QR/barcode can be copied; real entry control needs a reader that scans it and an online verifier that enforces expiry and revocation. |
| Apple Wallet contactless NFC/VAS pass | Conditional | Apple documents loyalty and membership contactless passes through Apple Pay VAS. The terminal/reader must be VAS-certified and the POS software must support VAS. No evidence yet shows that the Banasku gate reader or its controller supports this protocol. |
| NFC & SE Platform | Conditional; strongest Apple-provided route to investigate for secure-element presentment | Japan is eligible. Apple lists corporate badges for office-space access and merchant loyalty/rewards as separate use cases. A dog-run entry credential is not automatically a corporate badge; the operator would need to establish that its member program fits an eligible use case and receive Apple's approval. Japan requires iPhone XS or later with iOS 18.1 or later. The app, approved entitlement/product configuration, applet partner, and terminal supporting ISO 14443-4 and ISO 7816-4 are also required. |
| Core NFC tag reading | Not a phone-presented gate credential | Core NFC lets the iPhone read supported external tags. Reading an external FeliCa tag or its IDm does not issue a credential or make the iPhone present an arbitrary ID to the gate. |
| `CardSession` host card emulation | Not a Japan candidate under the current public docs | Apple's `CardSession` documentation describes HCE use cases in the European Economic Area and requires eligibility and managed entitlements. Treat it as unavailable for this Japan prototype unless Apple publishes a relevant change and grants eligibility. |
| Gate vendor mobile key / online card enrollment | Unconfirmed | No gate, controller, management-software model, API, SDK, or vendor statement has been supplied. |
| Existing iPhone card online enrollment | Unconfirmed | The card issuer, card technology, identifier mapping, and supported remote enrollment method are unknown. A Wallet serial number, member UUID, displayed card number, UID, and FeliCa IDm are not interchangeable. |

## Decision

Keep `CredentialProvider` and `LockRegistrationClient` behind app/API interfaces. A standard signed `.pkpass` was generated locally and added to the iOS Simulator. The existing Pass Type ID certificate matches the local private key. CloudApp signing secrets and API-based issuance remain unconfigured, so this does not verify the Rust Worker endpoint. Do not report gate compatibility or NFC presentment until each is tested at its own boundary. A successfully added QR pass is Wallet-flow evidence only.

## External checks needed

1. Confirm the iPhone model and iOS version used for testing.
2. Ask Apple whether the Banasku membership/entry use case fits merchant loyalty/rewards or another eligible NFC & SE Platform use case. Corporate Badge access is documented for office spaces and should not be assumed to cover a dog-run gate. Confirm Apple agreement, ABR onboarding, entitlement, applet, and partner requirements.
3. Ask the gate/reader provider whether its exact hardware supports Wallet VAS or NFC & SE ISO 14443-4 / ISO 7816-4 credentials and how those credentials are enrolled and revoked remotely.
4. Confirm whether the existing iPhone card issuer offers a supported online enrollment and identifier retrieval API.

NFC & SE Platform presentment requires an NFC reader and cannot be exercised in Simulator. A supported iPhone/iOS pair alone does not establish Apple entitlement or gate compatibility.

## Sources

- [Apple NFC & SE Platform requirements and territories](https://developer.apple.com/support/nfc-se-platform/)
- [Apple Wallet loyalty and membership passes](https://developer.apple.com/wallet/loyalty-passes/)
- [Apple Wallet pass building and signing](https://developer.apple.com/documentation/walletpasses/building-a-pass)
- [Apple Wallet pass distribution](https://developer.apple.com/documentation/walletpasses/distributing-and-updating-a-pass)
- [Apple Core NFC `CardSession`](https://developer.apple.com/documentation/corenfc/cardsession)
- [Apple Core NFC `currentIDm`](https://developer.apple.com/documentation/corenfc/nfcfelicatag/currentidm)
