# Collection and upload orchestration

shaide exposes the existing Control Panel APIs for collection CRUD, memberships,
file deletion, five-minute upload URLs and object-created notifications. Collection
mutations require an administrator. Administrators see all collections and member
IDs; users see their own memberships with `users: null`. Upload and file deletion
require an administrator or an active member of a collection with uploads enabled.

KC is optional. Without it, the server's ordinary APIs and collection metadata
listing remain available. Operations that need KC return 503 before reserving or
changing metadata. Local name, description, policy and membership updates use
durable collection fences. Administrative membership removal and a PATCH containing
only `organization_id` and `can_users_upload: false` use serialized metadata
transactions even while reservations or recovery work exist. They retain every
file and operation fence; current owner access is rechecked at upload and ingestion
admission. Grants, mixed updates and model changes still require the collection
fence. Public knowledge-center search is outside this change;
MCP continues through the gateway, KC-MCP and KC.

## Configuration

Set `KNOWLEDGE_CENTER_URL` to KC's fixed origin (no path, query, fragment or
credentials), `KNOWLEDGE_CENTER_INTERNAL_TOKEN` to a distinct secret with at least
32 bytes, and `S3_WEBHOOK_TOKEN` to the matching RustFS notification bearer secret
with at least 32 bytes. `KNOWLEDGE_CENTER_ALLOW_LOCAL_HTTP=true` explicitly permits
HTTP for an isolated local Compose network. HTTPS is required otherwise. The
internal and notification tokens must differ from each other and from `JWT_SECRET`.
Leave all three KC settings unset when KC is disabled.

All RustFS credentials, SDKs, notification provisioning and storage operations
belong to KC. KC configures RustFS to POST notifications to
`/v1/object-storage/event` with `Authorization: Bearer <S3_WEBHOOK_TOKEN>`.
Real RustFS support for this header must be verified with the pinned integration
image before publication; fake HTTP tests do not establish that capability.

## Upload transport

`POST /v1/object-storage/presigned-url` accepts the established
`organization_id`, `file_name`, `content_type` body and returns
`{"presigned_url":"/s3/collection-N/<identity>?..."}`. The file identity is
server-issued lowercase 64-hex, independent of content. The reservation is durable
and valid for 300 seconds. KC creates the signature. shaide stores only the SHA-256
fingerprint of the signed query, never the query or storage credentials.

The client PUTs bytes to that public relative URL with the original query,
`Content-Type` and exact `Content-Length`. shaide checks the reservation, canonical
path, declared size and current uploader access, then streams bytes to authenticated
`PUT /internal/v1/storage/upload/{collection}/{file_hash}` on its configured KC
origin. It forwards the original signed query and content headers. KC validates
and persists the upload to RustFS. Only PUT is exposed; redirects, arbitrary
upstream URLs and bucket browsing are unsupported. Both services enforce a 16 MiB
bound. Supported types are PDF, DOCX, PNG and JPEG.

shaide validates notifications against scoped metadata and trusted KC
`GET /internal/v1/storage/objects/{collection}/{file_hash}` results. Notification
size claims do not authorize ingestion. A notification arriving before the upload
response is recorded durably and processed after KC acknowledges the write.
Only one durable ingestion claim is allowed. Metadata becomes `processed` only
following a complete, valid KC ingestion acknowledgement. Duplicate completed
notifications do not index again. Ingest, upload and deletion failures are sanitized
and retained; acknowledgements with missing/invalid fields do not count as success.

## Durable operations and recovery

Migration 0004 adds `collection_operations` and `file_operations` without changing
existing file status names or deleting existing rows. Collections with legacy non-final file metadata receive a recovery fence because those rows cannot prove the remote outcome. Every SQL transaction ends
before an HTTP call. Collection fences block reservations and collection changes;
file fences block duplicate uploads, overlapping ingestion/deletion and destructive
collection changes. A disconnect or process restart may leave `active`, `uploading`,
`uploaded` or `ingesting` work. A failed/uncertain call leaves `recovery_required`.
Elapsed time never releases those fences. An expired reservation is not proof that
a remote writer stopped. File/collection deletion and model replacement remain
blocked until the recorded operation is settled.

Recovery is an explicit operator task, scoped to one operation ID and token. Do not
run blanket stale cleanup, retry remote mutations automatically, delete a file's
metadata first, or treat local request cancellation as a remote write fence.

1. Inspect the operation row, retained file status and update payload. Identify
   the exact collection/file and all possibly accepted KC work.
2. Establish that those public facade handlers, KC handlers and downstream writes have finished or been
   stopped and fenced. A timeout, elapsed lease or local restart is insufficient.
3. Reconcile the exact object's metadata and complete indexing/deletion outcome
   through KC. Object existence alone does not prove vectors are complete.
4. In a reviewed, scoped SQL transaction, settle the metadata to the verified
   outcome and remove only the matching operation token. Keep the fence if the
   remote outcome cannot be established. There is no public force-unlock API.

`DELETE /v1/rag/delete-stale-files` returns 409 when stale or recovery work exists;
it does not discard operation records or files. Operators must settle each one.
Downgrade migration 0004 only after all operations have been settled: dropping the
operation tables removes protection against overlapping remote work.

Populated collection embedding-model replacement returns 409. The frozen KC
collection DELETE removes stored files as well as vectors, so it cannot be used to
rebuild vectors while preserving documents. Empty collections can change models
through acknowledged KC delete/create under a durable fence. Collection create,
delete and both replacement phases require the frozen empty JSON object `{}`. Any
other successful JSON body is an uncertain outcome: metadata and the operation
fence are retained for reconciliation. A vector-only
replacement protocol requires a separately agreed KC contract; no new KC endpoint is assumed.

Native HTTP contract tests use explicit synthetic KC fakes and disposable SQLite
files. Real storage signature/header verification, extraction/embedding/Qdrant/MCP
E2E and container acceptance remain separate integration gates.
