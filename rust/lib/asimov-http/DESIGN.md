# HTTP integrity contracts

`asimov-http` owns HTTP digest and message-signature processing shared by clients
and servers. Cryptography uses SHA-256 and the protocol crate's Ed25519 keys.
Structured Fields parsing and serialization use `sfv` in RFC 8941 mode.

## Message context

- `Message` snapshots method/status, effective target URI, headers, and trailers.
  It has no dependency on a body type or async runtime.
- A server supplies the external absolute URI using trusted routing/proxy
  context. Origin-form HTTP/1.1 requests do not identify the scheme themselves.
- Header and trailer fields stay distinct. `tr` selects trailers; `req` selects
  the originating request in a response signature.
- `@request-target` needs the actual protocol-specific request-target form.
  `@target-uri` is preferable across HTTP versions and intermediaries.
- `sf` needs a declared field type; digest/signature dictionaries are predefined.
  `key`, `bs`, `tr`, and `req` are supported. Unsupported component parameters
  fail explicitly. Signature-parameter extensions retain their order and values.
- Signature metadata is limited to 64 KiB and 64 covered components. Parsed
  equivalent components are rejected even when parameter order differs.

## Digests

Content digests cover content-coded bytes, excluding transfer coding and HTTP
framing. Serialize/compress before hashing; verify before decoding or changing
bytes. Empty content has the SHA-256 digest of the empty byte sequence.

`Repr-Digest` requires the entire selected representation. `DigestOptions` can
generate it alongside `Content-Digest` for complete representations such as JSON
POSTs. Streaming adapters validate `Content-Digest`; a range or HEAD response's
representation digest must be checked against separately obtained representation
data, not against the partial/empty response body.

## Streaming responses

- `SigningBody` forwards DATA frames while hashing, then emits completion
  trailers. `Signature-Input` may precede the body or accompany the trailers.
  A source error emits no completion fields.
- Precomputed digests/signatures can be supplied in headers using buffered
  signing or the message-level API. The verifier supports either section for
  each signature field, including a header input with a trailer signature.
- HTTP/1.1 trailers require chunked framing and a client advertising
  `TE: trailers`. The signing adapter removes Content-Length, advertises its
  trailer names, and has an unknown body size even for empty content. HTTP/2
  transports the same final frame as trailing HEADERS.
- `VerifyingBody` forwards provisional DATA without buffering the content.
  It retains trailers until source EOF, verifies the digest and signature, and
  then forwards trailers. Missing trailers, tampering, transport failures, and
  frames after trailers produce one terminal error. Early drop is not success.
- Verification uses a caller-supplied time, normally header receipt. Signing
  parameters are fixed at stream creation. This lets long responses finish
  after their original validity window. A different timing profile can call the
  message-level verifier at its own validation instant.
- These adapters work with non-`Sync`, pinned bodies, including Axum bodies.
  Body errors and trailer frames must survive any intervening transport adapter.

## Authentication policy

The remote request profile covers method, target URI, Authorization, Accept,
Content-Type, and Content-Digest, with tag `asimov-remote-request-v1`. Requests
use an ASCII `ed25519:<base58-public-key>` key ID, a fresh 256-bit nonce, and a
60-second lifetime. Verification allows 30 seconds of clock skew. Remote clients
also cover optional representation/encoding/digest-preference fields they send.

A verifier receives an explicitly resolved key and its expected key ID. It
enforces algorithm consistency, required coverage, tag, and freshness. A
successful signature alone does not establish body integrity, key authorization,
or replay safety. Servers must verify content before execution, authorize the
key, and atomically reject consumed nonces until expiry plus skew. Replay-store
selection and middleware installation belong to the server application.

Response profiles select their required fields explicitly. Applications needing
request binding should cover appropriate `req` components, including the request
content digest or request signature to distinguish requests to the same endpoint.
Automatic signature negotiation and key discovery are outside this slice.
