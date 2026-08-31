use std::sync::OnceLock;

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{Error as PasswordHashError, SaltString},
};
use chrono::{DateTime, Utc};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, get_current_timestamp,
};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use crate::{config::get_environment_config, error::ShaideError};

const JWT_ISSUER: &str = "shaide";
const JWT_AUDIENCE: &str = "shaide-api";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserClaims {
    sub: String,
    iss: String,
    aud: String,
    iat: u64,
    exp: u64,
}

pub struct AuthService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    validation: Validation,
}

impl AuthService {
    pub const TOKEN_LIFETIME_SECONDS: u64 = 60 * 60;

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
        let expires_at = account_expiry
            .map(|expiry| {
                u64::try_from(expiry.timestamp()).map_err(|_| {
                    ShaideError::internal_server_error("Invalid account expiry".to_owned())
                })
            })
            .transpose()?
            .map_or(issued_at + Self::TOKEN_LIFETIME_SECONDS, |expiry| {
                expiry.min(issued_at + Self::TOKEN_LIFETIME_SECONDS)
            });
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

static AUTH_SERVICE: OnceLock<AuthService> = OnceLock::new();

pub fn get_auth_service() -> &'static AuthService {
    AUTH_SERVICE.get_or_init(|| AuthService::new(&get_environment_config().jwt_secret))
}
