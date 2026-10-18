use std::sync::OnceLock;

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{Error as PasswordHashError, SaltString},
};
use aws_lc_rs::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, get_current_timestamp,
};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use crate::{config::get_environment_config, error::ShaideError};

const JWT_ISSUER: &str = "shaide";
const JWT_AUDIENCE: &str = "shaide-api";
/// 256 bits, so a refresh token cannot be guessed and a plain digest is enough to store it.
const REFRESH_TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserClaims {
    sub: String,
    iss: String,
    aud: String,
    iat: u64,
    exp: u64,
}

/// A newly minted refresh token. `token` is handed to the client once; only `hash` is stored.
pub struct IssuedRefreshToken {
    pub token: String,
    pub hash: String,
    pub expires_at: DateTime<Utc>,
    pub expires_in: u64,
}

pub struct AuthService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    validation: Validation,
}

impl AuthService {
    pub const TOKEN_LIFETIME_SECONDS: u64 = 60 * 60;
    pub const REFRESH_TOKEN_LIFETIME_SECONDS: u64 = 30 * 24 * 60 * 60;

    pub fn new(jwt_secret: &str) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 5;
        validation.set_audience(&[JWT_AUDIENCE]);
        validation.set_issuer(&[JWT_ISSUER]);
        validation.set_required_spec_claims(&["sub", "iss", "aud", "iat", "exp"]);
        Self {
            encoding_key: EncodingKey::from_secret(jwt_secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(jwt_secret.as_bytes()),
            validation,
        }
    }

    pub async fn hash_password(&self, password: String) -> Result<String, ShaideError> {
        tokio::task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map(|hash| hash.to_string())
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| ShaideError::internal_server_error(error.to_string()))?
        .map_err(ShaideError::internal_server_error)
    }

    pub async fn verify_password(
        &self,
        password: String,
        password_hash: String,
    ) -> Result<(), ShaideError> {
        let result = tokio::task::spawn_blocking(move || {
            let hash = PasswordHash::new(&password_hash)?;
            Argon2::default().verify_password(password.as_bytes(), &hash)
        })
        .await
        .map_err(|error| ShaideError::internal_server_error(error.to_string()))?;
        match result {
            Ok(()) => Ok(()),
            Err(PasswordHashError::Password) => {
                Err(ShaideError::unauthorized("Invalid credentials".to_owned()))
            }
            Err(error) => Err(ShaideError::internal_server_error(error.to_string())),
        }
    }

    pub fn issue_access_token(
        &self,
        user_id: i64,
        account_expiry: Option<DateTime<Utc>>,
    ) -> Result<(String, u64), ShaideError> {
        let issued_at = get_current_timestamp();
        let expires_at = capped_expiry(issued_at, Self::TOKEN_LIFETIME_SECONDS, account_expiry)?;
        let claims = UserClaims {
            sub: user_id.to_string(),
            iss: JWT_ISSUER.to_owned(),
            aud: JWT_AUDIENCE.to_owned(),
            iat: issued_at,
            exp: expires_at,
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key)
            .map_err(|error| ShaideError::internal_server_error(error.to_string()))?;
        Ok((token, expires_at.saturating_sub(issued_at)))
    }

    /// Mints a refresh token that expires after [`Self::REFRESH_TOKEN_LIFETIME_SECONDS`], or at
    /// `account_expiry` when that comes first.
    pub fn issue_refresh_token(
        &self,
        account_expiry: Option<DateTime<Utc>>,
    ) -> Result<IssuedRefreshToken, ShaideError> {
        let mut bytes = [0u8; REFRESH_TOKEN_BYTES];
        SystemRandom::new().fill(&mut bytes).map_err(|_| {
            ShaideError::internal_server_error("Could not generate a refresh token".to_owned())
        })?;
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let issued_at = get_current_timestamp();
        let expires_at = capped_expiry(
            issued_at,
            Self::REFRESH_TOKEN_LIFETIME_SECONDS,
            account_expiry,
        )?;
        Ok(IssuedRefreshToken {
            hash: hash_refresh_token(&token),
            token,
            expires_at: i64::try_from(expires_at)
                .ok()
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
                .ok_or_else(|| {
                    ShaideError::internal_server_error("Invalid refresh token expiry".to_owned())
                })?,
            expires_in: expires_at.saturating_sub(issued_at),
        })
    }

    pub fn validate_access_token(&self, token: &str) -> Result<i64, ShaideError> {
        let claims = decode::<UserClaims>(token, &self.decoding_key, &self.validation)
            .map_err(|_| ShaideError::unauthorized("Invalid or expired access token".to_owned()))?
            .claims;
        claims
            .sub
            .parse()
            .map_err(|_| ShaideError::unauthorized("Invalid access token subject".to_owned()))
    }
}

/// Digest under which a refresh token is stored and looked up.
pub fn hash_refresh_token(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest(&SHA256, token.as_bytes()))
}

/// `issued_at + lifetime`, or the account expiry when that comes first, as a Unix timestamp.
fn capped_expiry(
    issued_at: u64,
    lifetime: u64,
    account_expiry: Option<DateTime<Utc>>,
) -> Result<u64, ShaideError> {
    let expires_at = issued_at + lifetime;
    let Some(account_expiry) = account_expiry else {
        return Ok(expires_at);
    };
    let account_expiry = u64::try_from(account_expiry.timestamp())
        .map_err(|_| ShaideError::internal_server_error("Invalid account expiry".to_owned()))?;
    Ok(expires_at.min(account_expiry))
}

static AUTH_SERVICE: OnceLock<AuthService> = OnceLock::new();

pub fn get_auth_service() -> &'static AuthService {
    AUTH_SERVICE.get_or_init(|| AuthService::new(&get_environment_config().jwt_secret))
}

/// Initializes the shared auth service with a fixed secret, so tests sign and
/// validate tokens without the environment configuration.
#[cfg(test)]
pub(crate) fn init_test_auth_service() -> &'static AuthService {
    AUTH_SERVICE.get_or_init(|| AuthService::new("test-only-jwt-secret-of-at-least-32-bytes"))
}
