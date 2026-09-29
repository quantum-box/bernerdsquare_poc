# Wallet signing fixtures

These DER certificates and the PKCS#8 key are disposable test-only fixtures for
CMS serialization, signature verification, and Pass Type ID signing-usage
checks. `test-wwdr.der` has an Apple-like subject name but was signed with a
throwaway test key; it is not an Apple certificate and intentionally fails the
production WWDR fingerprint check. Never configure these files as CloudApp
secrets or use them to issue a real Wallet pass.
