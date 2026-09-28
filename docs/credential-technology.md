# iPhone credential technology findings

Reviewed 2026-09-28 against Apple Developer documentation.

## Current classification

| Route | Classification | Evidence and condition |
|---|---|---|
| Apple Wallet contactless NFC/VAS pass | Conditional | Apple documents loyalty and membership contactless passes through Apple Pay VAS. The terminal/reader must be VAS-certified and the POS software must support VAS. No evidence yet shows that the Banasku gate reader or its controller supports this protocol. |
| NFC & SE Platform | Conditional; best Apple-supported route to investigate for secure-element access | Japan is an eligible territory, and the documented use cases include corporate badges. The applicant must fit a use-case eligibility rule, have the required agreement with Apple, onboard in Apple Business Register, request and receive the entitlement, and use a compatible terminal. Banasku's dog-run gate use case and relationship to a qualifying badge/access operator have not been established. |
| Core NFC tag reading | Not a phone-presented gate credential | Core NFC lets the iPhone read supported external tags. Reading an external FeliCa tag or its IDm does not issue a credential or make the iPhone present an arbitrary ID to the gate. |
| `CardSession` host card emulation | Not a Japan candidate under the current public docs | Apple's `CardSession` documentation describes HCE use cases in the European Economic Area and requires eligibility and managed entitlements. Treat it as unavailable for this Japan prototype unless Apple publishes a relevant change and grants eligibility. |
| Gate vendor mobile key / online card enrollment | Unconfirmed | No gate, controller, management-software model, API, SDK, or vendor statement has been supplied. |
| Existing iPhone card online enrollment | Unconfirmed | The card issuer, card technology, identifier mapping, and supported remote enrollment method are unknown. A Wallet serial number, member UUID, displayed card number, UID, and FeliCa IDm are not interchangeable. |

## Decision

Keep `CredentialProvider` and `LockRegistrationClient` behind app/API interfaces and run the prototype in mock mode. Do not select an NFC issuance path or report gate compatibility until Apple eligibility and the exact reader/controller protocol are confirmed. Mock completion is software-flow evidence only.

## External checks needed

1. Confirm the iPhone model and iOS version used for testing.
2. Confirm whether Banasku and its access-control relationship qualify for an Apple NFC & SE Platform use case; confirm Apple agreement, ABR onboarding, entitlement, applet, and partner requirements.
3. Ask the gate/reader provider whether its exact hardware supports Wallet VAS or NFC & SE ISO 14443-4 / ISO 7816-4 credentials and how those credentials are enrolled and revoked remotely.
4. Confirm whether the existing iPhone card issuer offers a supported online enrollment and identifier retrieval API.

## Sources

- [Apple NFC & SE Platform requirements and territories](https://developer.apple.com/support/nfc-se-platform/)
- [Apple Wallet loyalty and membership passes](https://developer.apple.com/wallet/loyalty-passes/)
- [Apple Core NFC `CardSession`](https://developer.apple.com/documentation/corenfc/cardsession)
- [Apple Core NFC `currentIDm`](https://developer.apple.com/documentation/corenfc/nfcfelicatag/currentidm)
