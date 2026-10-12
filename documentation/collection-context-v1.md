# Gateway collection context

`MCP_COLLECTION_CONTEXT_TARGETS` explicitly opts discovered targets into
collection-context-v1. It is independent of `MCP_SERVERS`: discovery alone
never opts a target in. Unset, blank or `[]` needs no signing or service secrets.
Partial or invalid configuration fails startup without echoing values.

```json
[{"server_id":"knowledge-center","url":"https://kc-mcp:8080","issuer":"shaide-gateway:local","kc_audience":"kc:knowledge-center","kid":"kc-context-v1","signing_key_file":"/run/secrets/kc_context_private.pem","gateway_token_file":"/run/secrets/kc_mcp_gateway_token"}]
```

These are the only fields. Configure at most 16 distinct DNS-label server IDs,
origin-only HTTPS URLs and absolute readable secret files. The selected
normalized discovery URL must equal the configured origin on every request;
a changed URL fails with `collection_context_target_mismatch`, never ordinary
forwarding. `MCP_COLLECTION_CONTEXT_ALLOW_LOCAL_HTTP=true` is an explicit
local-only transport option (default false). Operators must isolate local HTTP
links and avoid publishing receiver ports. The gateway cannot prove network
isolation from an origin string.

Use a dedicated RSA private key of at least 2048 bits and a separately generated
first-hop service token generated from at least 32 cryptographically random bytes,
encoded as canonical unpadded base64url. The file may end with exactly one LF;
CRLF, repeated newlines, padding and other whitespace are rejected. Validation
checks encoding and decoded length, and cannot establish randomness. `kid` uses
a 1..63 lowercase DNS label (letters/digits/hyphens, no leading/trailing hyphen). Neither may reuse
`JWT_SECRET`. Receivers receive only the corresponding public verification
key; key and kid rotation requires coordinated configuration. No dynamic key
fetching. Do not put secrets inline in configuration or commit keys.

Participating targets expose only `/v1/mcp/proxy/{server_id}/mcp`, forwarded to
exact `/mcp`, with POST/GET/DELETE and no query. Redirects fail; generic internal
paths, encoded paths and wildcard suffixes cannot use this capability. Ordinary
MCP targets retain user-only auth, caller downstream-key forwarding and original
bodies. Reserved `X-Shaide-*` / `X-KC-*` headers are stripped for every target.
Admins may discover and proxy only participating targets; regular users retain
the existing discovery list. Every accepted authorization records one end-user
API usage row, including later transport/context failures. Embedding usage and
quota remain attributed to the KC service account, with no per-user billing claim.

The gateway authenticates once and reads current identity, role, account expiry,
complete collection/model references and optional tools/list catalog in a single
SQLite transaction. Collections map to legacy `organizations`; user memberships
are many-to-many, zero grants none, admins read all current rows. Membership,
account and role changes affect the next HTTP request. Sessions and tokens do not
cache privileges. Catalog includes only existing name/description, full file
counts and up to three newest processed filenames. It reads no document content,
provider URL, storage object key or membership user list.

`X-Shaide-Collection-Context` contains a compact RS256 JWS with header
`alg=RS256`, `typ=shaide-collection-context+jwt`, configured `kid`. Claims are
`v,iss,aud,target,sub,role,iat,exp,jti,request_id,capability,collections,catalog`.
`aud` is exactly `["kc-mcp:<server_id>","<configured kc_audience>"]`, `sub` and
`request_id` are positive signed-i64 decimal strings; collection IDs remain
lossless JSON integers. Capability is `kc-read-v1`; each model is `{name,vector_size}`.
Every HTTP request receives a fresh 30-second snapshot. Upstream send/read is
cancelled at `claims.exp` without grace or refresh; the response body rejects its
first poll after expiry. **Pending downstream socket-write cancellation remains
an open acceptance blocker:** once a frame is handed to Hyper, its user-space
write buffer can remain pending under backpressure and write after expiry. The
route cannot cancel those writes. Already OS/client-owned bytes cannot be recalled.
A request/stream-scoped transport ownership change requires separate review; the
approved delivery deadline has not been relaxed or met by this implementation.

Participating responses require finite `application/json` for nonempty success
bodies, with valid JSON; SSE is explicitly rejected, including empty SSE.
Empty 202/204 acknowledgments are accepted without a media type. Upstream
401/403/405/413 retain their statuses with fixed sanitized JSON errors; 405 may
retain a validated standard-method `Allow` list. Other upstream errors become
503. Successful responses retain only content type, MCP session/protocol headers
and `Cache-Control: private, no-store`; arbitrary bodies/headers from upstream
errors and credentials/cookies are never forwarded.

Supported limits: 64 complete collections, 12000 decoded claim bytes, 16384
compact JWS bytes, 32768 aggregate header bytes, 262144 body bytes. Core scope
never truncates: overflow is HTTP 413 `collection_context_too_large`. Invalid
persisted model references fail HTTP 503 `collection_context_metadata_invalid`.
Display catalog is an ascending prefix within 4096 UTF-8 JSON bytes, with exact
`omitted_collections`, `omitted_documents` and `text_truncated`. Names/filenames
are at most 96 Unicode characters and descriptions 240. Display never authorizes.
Receiver tool descriptions remain independently bounded to 512 o200k tokens.

Incoming identity/scope headers and reserved request / params `_meta` members
are stripped before injection; unrelated metadata is preserved. Duplicate JSON
members and batches fail before forwarding. Search tool arguments are independently
validated by the receiver against the finalized query/max_results/min_score schema.
No query binding or replay store is claimed: a stolen envelope plus service
credential can replay reads within the 30-second lifetime. Broad KC internal
credentials are separate and never held by this gateway sender path.

The SHD-1069 metadata DAOs, migration 0003 and DTOs are reused from an unaccepted
public-server draft. Its existing persistence tests, including eight simultaneous
claims, remain required. The new `DbConn::collection_context_snapshot` helper is
read-only, neither authenticates tokens nor accounts usage, and requires callers
to recheck the returned account before issuing context. Independent metadata
acceptance, parent review, actual private receiver interoperability and deployment
header-limit probes remain separate gates.


Generate first-hop credentials using a cryptographically secure source, for
example this command writes 32 random bytes as unpadded base64url with one LF
without printing the token (run in an operator-owned secret directory):

```sh
(umask 077; python3 -c 'import base64,os,secrets; fd=os.open("kc_mcp_gateway_token", os.O_WRONLY|os.O_CREAT|os.O_EXCL, 0o600); f=os.fdopen(fd,"wb"); f.write(base64.urlsafe_b64encode(secrets.token_bytes(32)).rstrip(b"=")+b"\n"); f.close()')
```

This creates a new mode-0600 file and refuses every existing path, including
regular files, symlinks and dangling symlinks, preserving their content and mode.
Credential rotation is a separate, explicit operator procedure outside this
creation example.

Synthetic loopback HTTP fixtures establish local behavior only. Acceptance still
requires the intended MCP client through the gateway to the actual finalized
KC-MCP/KC receivers, including GET/DELETE 405 handling, notification 202/204,
strict verification, service credential isolation, TLS/local isolation, actual
multi-hop limits and the unresolved downstream delivery deadline.
