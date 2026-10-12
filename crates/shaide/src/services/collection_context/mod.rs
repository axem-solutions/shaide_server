//! Public-side collection-context-v1 sender. Independent from private receivers.
use axum::body::{Body, to_bytes};
use base64::{Engine, engine::general_purpose::STANDARD};
use http::{HeaderMap, Method, StatusCode, Uri};
use std::{collections::BTreeMap, path::Path, sync::Arc};

use crate::{
    error::ShaideError,
    middlewares::{AccessRequirement, ensure_access},
};
use serde::{Deserialize, Serialize};
use shaide_db::collection_context::CollectionContextSnapshot;

pub const CONTEXT_HEADER: &str = "x-shaide-collection-context";
pub const BODY_LIMIT: usize = 262144;
pub const HEADER_LIMIT: usize = 32768;
const CLAIMS_LIMIT: usize = 12000;
const ENVELOPE_LIMIT: usize = 16384;

pub fn failure(status_code: StatusCode, code: &'static str) -> ShaideError {
    ShaideError::RequestRejection {
        status_code,
        message: code.into(),
        code: code.into(),
    }
}
fn oversized() -> ShaideError {
    failure(
        StatusCode::PAYLOAD_TOO_LARGE,
        "collection_context_too_large",
    )
}
fn invalid() -> ShaideError {
    failure(
        StatusCode::BAD_REQUEST,
        "collection_context_invalid_request",
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetConfig {
    server_id: String,
    url: String,
    issuer: String,
    kc_audience: String,
    kid: String,
    signing_key_file: String,
    gateway_token_file: String,
}

pub struct ContextTarget {
    pub server_id: String,
    pub origin: String,
    issuer: String,
    kc_audience: String,
    kid: String,
    key: aws_lc_rs::rsa::KeyPair,
    token: http::HeaderValue,
}
impl std::fmt::Debug for ContextTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContextTarget([redacted])")
    }
}

#[derive(Clone, Default)]
pub struct ContextTargets(pub BTreeMap<String, Arc<ContextTarget>>);
impl std::fmt::Debug for ContextTargets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContextTargets([redacted])")
    }
}
fn ascii(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|b| (33..=126).contains(&b))
}
fn label(value: &str) -> bool {
    (1..=63).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}
pub fn origin(raw: &str, local_http: bool) -> Result<String, &'static str> {
    let url = url::Url::parse(raw).map_err(|_| "invalid context target origin")?;
    if !matches!(url.scheme(), "https" | "http")
        || (url.scheme() == "http" && !local_http)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || raw.contains('\\')
        || raw.contains('%')
        || raw.trim() != raw
        || raw.contains("/../")
        || raw.contains("/./")
        || raw.ends_with("/..")
        || raw.ends_with("/.")
    {
        return Err("invalid context target origin");
    }
    Ok(url.as_str().trim_end_matches('/').into())
}
fn secret_file(path: &str, max: u64) -> Result<Vec<u8>, &'static str> {
    if !Path::new(path).is_absolute() {
        return Err("invalid context secret file");
    }
    let mut file = std::fs::File::open(path).map_err(|_| "unreadable context secret file")?;
    if !file
        .metadata()
        .map_err(|_| "unreadable context secret file")?
        .is_file()
    {
        return Err("invalid context secret file");
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    (&mut file)
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "unreadable context secret file")?;
    if bytes.len() as u64 > max {
        return Err("oversized context secret file");
    }
    Ok(bytes)
}
impl ContextTargets {
    pub fn parse(
        raw: Option<&str>,
        local_http: Option<&str>,
        user_secret: &str,
    ) -> Result<Self, &'static str> {
        let local_http = match local_http {
            None | Some("false") => false,
            Some("true") => true,
            _ => return Err("invalid context local HTTP option"),
        };
        let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
            return Ok(Self::default());
        };
        if raw.len() > 65536 {
            return Err("oversized context configuration");
        }
        let strict = serde_json::from_str::<StrictValue>(raw)
            .map_err(|_| "invalid context configuration")?;
        let configs: Vec<TargetConfig> =
            serde_json::from_value(strict.0).map_err(|_| "invalid context configuration")?;
        if configs.len() > 16 {
            return Err("too many context targets");
        }
        let mut targets = BTreeMap::new();
        for config in configs {
            if !label(&config.server_id)
                || !ascii(&config.issuer)
                || !ascii(&config.kc_audience)
                || !label(&config.kid)
                || config.kc_audience == format!("kc-mcp:{}", config.server_id)
                || targets.contains_key(&config.server_id)
            {
                return Err("invalid context configuration");
            }
            let origin = origin(&config.url, local_http)?;
            let pem = secret_file(&config.signing_key_file, 32768)?;
            if pem == user_secret.as_bytes()
                || std::str::from_utf8(&pem)
                    .ok()
                    .is_some_and(|p| p.trim() == user_secret.trim())
            {
                return Err("context key must be dedicated");
            }
            let pem_text = std::str::from_utf8(&pem).map_err(|_| "invalid context signing key")?;
            let (begin, end, pkcs8) = if pem_text.starts_with("-----BEGIN PRIVATE KEY-----") {
                (
                    "-----BEGIN PRIVATE KEY-----",
                    "-----END PRIVATE KEY-----",
                    true,
                )
            } else {
                (
                    "-----BEGIN RSA PRIVATE KEY-----",
                    "-----END RSA PRIVATE KEY-----",
                    false,
                )
            };
            let encoded = pem_text
                .strip_prefix(begin)
                .and_then(|s| s.trim().strip_suffix(end))
                .ok_or("invalid context signing key")?;
            let der = STANDARD
                .decode(encoded.split_whitespace().collect::<String>())
                .map_err(|_| "invalid context signing key")?;
            let pair = if pkcs8 {
                aws_lc_rs::rsa::KeyPair::from_pkcs8(&der)
            } else {
                aws_lc_rs::rsa::KeyPair::from_der(&der)
            }
            .map_err(|_| "invalid context signing key")?;
            if pair.public_modulus_len() < 256 || pair.public_modulus_len() > 1024 {
                return Err("unsupported context signing key size");
            }
            let token = secret_file(&config.gateway_token_file, 4096)?;
            let token = std::str::from_utf8(&token).map_err(|_| "invalid context gateway token")?;
            // Exactly one optional LF file terminator; all other whitespace
            // (including CRLF) is rejected by the canonical base64url decoder.
            let token = token.strip_suffix('\n').unwrap_or(token);
            let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(token)
                .map_err(|_| "invalid context gateway token")?;
            // Length/encoding cannot establish randomness of the source bytes.
            if decoded.len() < 32 || token == user_secret {
                return Err("invalid context gateway token");
            }
            let mut token = http::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| "invalid context gateway token")?;
            token.set_sensitive(true);
            let target = ContextTarget {
                server_id: config.server_id.clone(),
                origin,
                issuer: config.issuer,
                kc_audience: config.kc_audience,
                kid: config.kid,
                key: pair,
                token,
            };
            targets.insert(config.server_id, Arc::new(target));
        }
        Ok(Self(targets))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingReference {
    pub name: String,
    pub vector_size: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionGrant {
    pub collection_id: i64,
    pub embedding_model: EmbeddingReference,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub collection_id: i64,
    pub name: String,
    pub description: String,
    pub uploaded_documents: i64,
    pub searchable_documents: i64,
    pub document_names: Vec<String>,
    pub omitted_documents: i64,
    pub text_truncated: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
    pub omitted_collections: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    pub v: u8,
    pub iss: String,
    pub aud: [String; 2],
    pub target: String,
    pub sub: String,
    pub role: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    pub request_id: String,
    pub capability: String,
    pub collections: Vec<CollectionGrant>,
    pub catalog: Option<Catalog>,
}
fn shorten(text: &str, chars: usize, truncated: &mut bool) -> String {
    if text.chars().count() > chars {
        *truncated = true;
    }
    text.chars().take(chars).collect()
}
impl ContextTarget {
    pub fn claims(
        &self,
        snapshot: CollectionContextSnapshot,
        request_id: i64,
        catalog: bool,
        now: i64,
    ) -> Result<Claims, ShaideError> {
        ensure_access(&snapshot.user, AccessRequirement::Any)?;
        if snapshot.collections.len() > 64 {
            return Err(oversized());
        }
        if snapshot.user.id <= 0 || request_id <= 0 || now < 0 {
            return Err(invalid());
        }
        let mut collections = Vec::new();
        let mut entries = Vec::new();
        let mut previous = 0;
        for row in snapshot.collections {
            let name = row.model_name.ok_or_else(|| {
                failure(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "collection_context_metadata_invalid",
                )
            })?;
            let size = row.vector_size.unwrap_or(0);
            if row.collection_id <= previous
                || name.trim().is_empty()
                || name.chars().any(char::is_control)
                || name.chars().count() > 256
                || !(1..=65536).contains(&size)
            {
                return Err(failure(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "collection_context_metadata_invalid",
                ));
            }
            previous = row.collection_id;
            collections.push(CollectionGrant {
                collection_id: row.collection_id,
                embedding_model: EmbeddingReference {
                    name,
                    vector_size: size,
                },
            });
            if catalog {
                let mut truncated = false;
                let names: Vec<_> = row
                    .document_names
                    .iter()
                    .map(|s| shorten(s, 96, &mut truncated))
                    .collect();
                if row.searchable_documents < names.len() as i64
                    || row.uploaded_documents < row.searchable_documents
                    || names.len() > 3
                {
                    return Err(failure(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "collection_context_metadata_invalid",
                    ));
                }
                entries.push(CatalogEntry {
                    collection_id: row.collection_id,
                    name: shorten(&row.name, 96, &mut truncated),
                    description: shorten(&row.description, 240, &mut truncated),
                    uploaded_documents: row.uploaded_documents,
                    searchable_documents: row.searchable_documents,
                    omitted_documents: row.searchable_documents - names.len() as i64,
                    document_names: names,
                    text_truncated: truncated,
                });
            }
        }
        let mut claims = Claims {
            v: 1,
            iss: self.issuer.clone(),
            aud: [
                format!("kc-mcp:{}", self.server_id),
                self.kc_audience.clone(),
            ],
            target: self.server_id.clone(),
            sub: snapshot.user.id.to_string(),
            role: String::from(snapshot.user.role),
            iat: now,
            exp: now + 30,
            jti: fresh_jti(),
            request_id: request_id.to_string(),
            capability: "kc-read-v1".into(),
            collections,
            catalog: catalog.then_some(Catalog {
                entries,
                omitted_collections: 0,
            }),
        };
        loop {
            let catalog_bytes = claims
                .catalog
                .as_ref()
                .map(|c| serde_json::to_vec(c).unwrap().len())
                .unwrap_or(0);
            let claims_bytes = serde_json::to_vec(&claims).map_err(|_| invalid())?.len();
            let envelope = self.sign(&claims)?;
            if catalog_bytes <= 4096
                && claims_bytes <= CLAIMS_LIMIT
                && envelope.len() <= ENVELOPE_LIMIT
            {
                return Ok(claims);
            }
            if let Some(catalog) = &mut claims.catalog
                && catalog.entries.pop().is_some()
            {
                catalog.omitted_collections += 1;
                continue;
            }
            return Err(oversized());
        }
    }
    pub fn sign(&self, claims: &Claims) -> Result<String, ShaideError> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header =
            serde_json::json!({"alg":"RS256","typ":"shaide-collection-context+jwt","kid":self.kid});
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).map_err(|_| invalid())?);
        let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).map_err(|_| invalid())?);
        let input = format!("{header}.{claims}");
        let mut signature = vec![0; self.key.public_modulus_len()];
        self.key
            .sign(
                &aws_lc_rs::signature::RSA_PKCS1_SHA256,
                &aws_lc_rs::rand::SystemRandom::new(),
                input.as_bytes(),
                &mut signature,
            )
            .map_err(|_| {
                failure(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "collection_context_signing_failed",
                )
            })?;
        Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    pub fn inject(&self, headers: &mut HeaderMap, envelope: &str) -> Result<(), ShaideError> {
        headers.insert("authorization", self.token.clone());
        let mut value = http::HeaderValue::from_str(envelope).map_err(|_| oversized())?;
        value.set_sensitive(true);
        headers.insert(CONTEXT_HEADER, value);
        if header_bytes(headers) > HEADER_LIMIT {
            return Err(oversized());
        }
        Ok(())
    }
}
pub fn header_bytes(headers: &HeaderMap) -> usize {
    headers
        .iter()
        .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
        .sum()
}
pub fn clean_headers(headers: &mut HeaderMap, participating: bool) {
    let nominations: Vec<String> = headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    for name in nominations {
        headers.remove(name);
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
    let remove: Vec<_> = headers
        .keys()
        .filter(|n| {
            n.as_str().starts_with("x-shaide-")
                || n.as_str().starts_with("x-kc-")
                || (participating
                    && matches!(
                        n.as_str(),
                        "authorization"
                            | "x-mcp-authorization"
                            | "x-user-id"
                            | "x-username"
                            | "x-role"
                            | "x-organization-id"
                            | "x-organization-ids"
                            | "x-collection-id"
                            | "x-collection-ids"
                    ))
        })
        .cloned()
        .collect();
    for name in remove {
        headers.remove(name);
    }
    headers.remove("host");
}
pub fn exact_route(uri: &Uri, method: &Method, server: &str) -> Result<(), ShaideError> {
    if uri.path() != format!("/v1/mcp/proxy/{server}/mcp")
        || uri.query().is_some()
        || !matches!(*method, Method::POST | Method::GET | Method::DELETE)
    {
        return Err(failure(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(())
}
fn reserved(key: &str) -> bool {
    key.starts_with("shaide/")
        || key.starts_with("kc/")
        || matches!(
            key,
            "user_id"
                | "username"
                | "role"
                | "organization_id"
                | "organization_ids"
                | "collection_id"
                | "collection_ids"
                | "scopes"
                | "collection_context"
                | "context"
        )
}
pub async fn clean_body(body: Body, method: &Method) -> Result<(Body, bool), ShaideError> {
    let bytes = to_bytes(body, BODY_LIMIT).await.map_err(|_| oversized())?;
    if *method != Method::POST {
        if !bytes.is_empty() {
            return Err(invalid());
        }
        return Ok((Body::empty(), false));
    }
    let mut value = serde_json::from_slice::<StrictValue>(&bytes)
        .map_err(|_| invalid())?
        .0;
    let object = value.as_object_mut().ok_or_else(invalid)?;
    if object.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0")
        || object.get("method").and_then(|v| v.as_str()).is_none()
    {
        return Err(invalid());
    }
    let catalog = object["method"] == "tools/list";
    if let Some(meta) = object.get_mut("_meta").and_then(|v| v.as_object_mut()) {
        meta.retain(|key, _| !reserved(key));
    }
    if let Some(params) = object.get_mut("params").and_then(|v| v.as_object_mut())
        && let Some(meta) = params.get_mut("_meta").and_then(|v| v.as_object_mut())
    {
        meta.retain(|key, _| !reserved(key));
    }
    // The receiver validates tool arguments against the finalized schema;
    // gateway does not rewrite query / arguments or turn metadata into grants.
    Ok((
        Body::from(serde_json::to_vec(&value).map_err(|_| invalid())?),
        catalog,
    ))
}

// Reject duplicate JSON object members at every depth before projection. A
// serde_json::Value alone silently replaces duplicates, making spoof handling
// ambiguous across parsers.
struct StrictValue(serde_json::Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| StrictValue(v.into()))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(serde_json::Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(serde_json::Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(v) = a.next_element::<StrictValue>()? {
                    values.push(v.0);
                }
                Ok(StrictValue(values.into()))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut map = serde_json::Map::new();
                while let Some((k, v)) = a.next_entry::<String, StrictValue>()? {
                    if map.insert(k, v.0).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON member"));
                    }
                }
                Ok(StrictValue(map.into()))
            }
        }
        d.deserialize_any(Visitor)
    }
}

pub(crate) fn fresh_jti() -> String {
    use rand_core::RngCore;
    let mut bytes = [0u8; 16];
    rand_core::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests;
