use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{pkcs8::DecodePrivateKey, Signer};
use rsa::{pkcs1::DecodeRsaPrivateKey, signature::SignatureEncoding};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::error;
use crate::errors::MarketDataError;

/// Stored as one SecretStore value; never derive Debug for credentials or keys.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FutuCredentials {
    pub app_key_id: String,
    pub private_key: String,
}

impl FutuCredentials {
    pub fn validate(&self) -> Result<(), MarketDataError> {
        self.validate_app_key_id()?;
        SigningKey::parse(&self.private_key).map(|_| ())
    }

    pub(super) fn validate_app_key_id(&self) -> Result<(), MarketDataError> {
        if self.app_key_id.trim().is_empty()
            || reqwest::header::HeaderValue::from_str(&self.app_key_id).is_err()
        {
            return Err(error("Invalid AppKeyId"));
        }
        Ok(())
    }
}

pub(super) enum SigningKey {
    Ed25519(Box<ed25519_dalek::SigningKey>),
    Rsa(Box<rsa::pkcs1v15::SigningKey<Sha256>>),
}

impl SigningKey {
    pub fn parse(pem: &str) -> Result<Self, MarketDataError> {
        if let Ok(key) = ed25519_dalek::SigningKey::from_pkcs8_pem(pem) {
            return Ok(Self::Ed25519(Box::new(key)));
        }
        rsa::RsaPrivateKey::from_pkcs8_pem(pem)
            .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(pem))
            .map(|key| Self::Rsa(Box::new(rsa::pkcs1v15::SigningKey::<Sha256>::new(key))))
            .map_err(|_| {
                error(
                    "Invalid private key: use unencrypted Ed25519 PKCS#8 or RSA PKCS#8/PKCS#1 PEM",
                )
            })
    }

    pub fn sign(&self, message: &[u8]) -> Result<String, MarketDataError> {
        let bytes = match self {
            Self::Ed25519(key) => key.sign(message).to_bytes().to_vec(),
            Self::Rsa(key) => key
                .try_sign(message)
                .map_err(|_| error("Unable to sign request"))?
                .to_vec(),
        };
        Ok(STANDARD.encode(bytes))
    }
}

pub(super) fn signing_message(timestamp: i64, request: &reqwest::Request, body: &[u8]) -> String {
    let digest = if body.is_empty() {
        String::new()
    } else {
        format!("{:x}", Sha256::digest(body))
    };
    format!(
        "{}\n{}\n{}\n{}\n{}",
        timestamp,
        request.method(),
        request.url().path(),
        request.url().query().unwrap_or_default(),
        digest
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{pkcs8::EncodePrivateKey, Verifier};

    #[test]
    fn futu_signs_exact_get_and_post_bytes() {
        let client = reqwest::Client::new();
        let get = client
            .get("https://webapi.futunn.com/api/v1.0/quote/trading-days")
            .query(&[
                ("market", "HK"),
                ("start", "2025-12-22"),
                ("end", "2025-12-26"),
            ])
            .build()
            .unwrap();
        assert_eq!(signing_message(1782357937000, &get, b""),
            "1782357937000\nGET\n/api/v1.0/quote/trading-days\nmarket=HK&start=2025-12-22&end=2025-12-26\n");
        let body = br#"{"code_list":["HK.00700"]}"#;
        let post = client
            .post("https://webapi.futunn.com/api/v1.0/quote/stock-quote")
            .query(&[("q", "a +中")])
            .body(body.to_vec())
            .build()
            .unwrap();
        let message = signing_message(123, &post, body);
        assert!(message.contains("q=a+%2B%E4%B8%AD\n"));
        assert!(message.ends_with(&format!("{:x}", Sha256::digest(body))));
        assert_ne!(message, signing_message(123, &post, b"{ }"));
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let pem = key.to_pkcs8_pem(Default::default()).unwrap();
        let signer = SigningKey::parse(&pem).unwrap();
        let signature = STANDARD
            .decode(signer.sign(message.as_bytes()).unwrap())
            .unwrap();
        key.verifying_key()
            .verify(
                message.as_bytes(),
                &ed25519_dalek::Signature::from_slice(&signature).unwrap(),
            )
            .unwrap();
        assert!(SigningKey::parse("not a key").is_err());
    }

    #[test]
    fn futu_rsa_pkcs8_and_pkcs1_signatures_verify() {
        use rsa::{pkcs1::EncodeRsaPrivateKey, rand_core::OsRng};
        let key = rsa::RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
        let public = rsa::pkcs1v15::VerifyingKey::<Sha256>::new(key.to_public_key());
        for pem in [
            key.to_pkcs8_pem(Default::default()).unwrap(),
            key.to_pkcs1_pem(Default::default()).unwrap(),
        ] {
            let signer = SigningKey::parse(&pem).unwrap();
            let bytes = STANDARD
                .decode(signer.sign(b"fixed request").unwrap())
                .unwrap();
            let signature = rsa::pkcs1v15::Signature::try_from(bytes.as_slice()).unwrap();
            public.verify(b"fixed request", &signature).unwrap();
            assert!(public.verify(b"changed request", &signature).is_err());
        }
    }
}
