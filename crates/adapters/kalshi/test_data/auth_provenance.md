# Authentication Fixtures

These keys are public test material. They have no Kalshi account or trading access.

The RSA 2048-bit and P-256 test keys are generated locally with OpenSSL 3.0.13.
The RSA PKCS#1 file encodes the same key as the RSA PKCS#8 file. Private keys are stored as DER;
tests construct their PEM envelopes at runtime.
The 32-byte Ed25519 seed contains every byte in the interval `[0x00, 0x1f]`, in ascending order.
The key uses PKCS#8 encoding.
The public DER files come from these test keys.

`auth_vectors.json` contains original canonical request payloads and exact Ed25519 signatures
created with OpenSSL `pkeyutl -sign -rawin`. No SDK source or packaged fixtures are copied.
The encrypted key is the Ed25519 test key encrypted with OpenSSL 3.0.13 and the public test password
`public-test-only`. It tests rejection only.
