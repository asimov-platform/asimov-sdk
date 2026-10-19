# HTTP integrity and proxy contracts

`asimov-http` owns HTTP digest and message-signature processing shared by clients
and servers. Cryptography uses SHA-256 and `iroh-base`'s Ed25519 keys, shared
with the protocol crate without depending on its networking implementation.
Structured Fields parsing and serialization use `sfv` in RFC 8941 mode.
Client proxy policy is a separate capability; service addressing, credentials,
timeouts, and connection-pool ownership remain with callers.

## Capability boundaries

- Digest hashing and encoding always work with `no_std` and alloc.
- `integrity` bundles the existing signature, field, and body APIs and enables
  `std`. It is included in `all` and the default features. Direct consumers
  previously requesting only `std` must now request `integrity` for these APIs.
- `proxy` adds alloc-only options, URL validation, and dedicated proxy errors.
- `system-proxy` adds std-dependent native/environment discovery. It does not
  activate signature/key dependencies or an HTTP client backend.
- `reqwest` adds the origin-scoped client-builder adapter and SOCKS4 system-DNS
  workaround. It enables `system-proxy`; integrity-only builds do not gain it.
- `hyper` adds `OriginProxy::proxy_for_hyper`, applying NO_PROXY with the same
  `hyper-util` matcher used by reqwest and the shared wildcard-IP fix. It enables
  `system-proxy`, without enabling reqwest or integrity. The caller owns its
  connector protocols, TLS, timeouts, and pools.
- Remote re-exports the shared options and maps proxy errors to its existing
  variants. Its `TransportOptions`, timeouts, and execution lifecycle stay local.
- The SDK forwards `std,http` to `integrity` for compatibility; `http-proxy` and
  `http-client` expose proxy capabilities. With defaults disabled, those flags
  can be used without the SDK's `std`/integrity bundle.

## Origin-scoped proxy policy

`OriginProxy::resolve` binds a snapshot to one scheme/host/effective-port tuple.
It validates explicit endpoints before bypass evaluation or native reads. Native
bypass syntax is evaluated for that origin; the effective environment-syntax
NO_PROXY list remains alongside the endpoint for the backend to apply. Thus
`proxy_url()` is a candidate, not permission to skip `no_proxy()` matching.

The reqwest adapter replaces existing proxy configuration, disables automatic
discovery and redirects, and preserves TLS, timeout, and service-header settings.
Its result is a raw reqwest builder: callers must restrict subsequent request
URLs to `origin()`, using `check_origin()` when needed. The helper does not guard
arbitrary requests after the client is built. Remote already validates operation
URLs and constructs discovery URLs on its configured origin.

Do not reuse the resolved policy for another origin, including a redirect target.
A later multi-origin adapter needs per-destination resolution, explicit failure
handling, and appropriate pool reuse. A synchronous `Proxy::custom` callback
cannot represent discovery errors or ordered PAC fallbacks. Keep the initial
adapter origin-scoped rather than silently weakening those contracts.

Proxy options, discovery, and configuration errors have no dependency on remote
execution. Native fixtures and HTTP/SOCKS interoperability tests live here;
remote retains tests for API compatibility, shared discovery/execution routing,
bearer/signature separation, and transport budgets.

OpenAI forwarding defaults to automatic discovery and binds its Hyper connector
to the OpenRouter origin. `ProxyConfig::from_env` uses shared uppercase-first
precedence and native discovery; `from_options` supports local bypass overrides.
Parsed or manually constructed routes are explicit choices. The connector
supports HTTP(S) CONNECT and SOCKS5/5h; selected SOCKS4/4a endpoints fail explicitly.

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
