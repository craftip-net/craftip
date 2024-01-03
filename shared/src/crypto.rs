use std::fmt;
use ring::{rand, signature, digest};
use ring::rand::SecureRandom;
use ring::signature::KeyPair;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

const BASE36_ENCODER_STRING: &str = "0123456789abcdefghijklmnopqrstuvwxyz";
const PREFIX: &str = "CraftIPServerHost";
const HOSTNAME_LENGTH: usize = 20;

pub type ChallengeDataType = [u8; 64];
pub type SignatureDataType = [u8; 64];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerPrivateKey {
    #[serde(with = "BigArray")]
    key: [u8; 83],
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ServerPublicKey {
    key: [u8; 32],
}
fn create_challenge(data: &[u8]) -> Vec<u8> {
    [PREFIX.as_bytes(), data].concat()
}
impl Default for ServerPrivateKey {
    /// Generate a random key for the server.
    fn default() -> Self {
        let rng = rand::SystemRandom::new();
        let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key = pkcs8_bytes.as_ref();
        let mut result = [0u8; 83];
        result.copy_from_slice(key);
        Self {
            key: result
        }
    }
}

impl TryFrom<&str> for ServerPrivateKey {
    type Error = &'static str;

    /// decodes server from HEX string
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let key_vec = hex::decode(value).map_err(|_| "invalid hex string")?;
        if key_vec.len() != 83 {
            return Err("invalid length");
        }
        let mut key = [0u8; 83];
        key.copy_from_slice(&key_vec);
        Ok(Self {
            key
        })
    }
}
impl ServerPrivateKey {
    pub fn sign(&self, data: &[u8]) -> SignatureDataType {
        let data = create_challenge(data);
        let key_pair = signature::Ed25519KeyPair::from_pkcs8(self.key.as_ref()).unwrap();
        let mut result: SignatureDataType = [0u8; 64];
        let signature = key_pair.sign(data.as_ref());
        result.copy_from_slice(signature.as_ref());
        result
    }
    pub fn get_public_key(&self) -> ServerPublicKey {
        let key_pair = signature::Ed25519KeyPair::from_pkcs8(self.key.as_ref()).unwrap();
        let mut result = [0u8; 32];
        result.copy_from_slice(key_pair.public_key().as_ref());
        ServerPublicKey {
            key: result
        }
    }
}

impl fmt::Display for ServerPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", hex::encode(self.key.as_ref()))
    }
}
// convert from string
impl TryFrom<&str> for ServerPublicKey {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut result = [0u8; 32];
        let bytes = base_x::decode(BASE36_ENCODER_STRING, value)
            .map_err(|_| "invalid base36 string")?;
        if bytes.len() != 32 {
            return Err("invalid length");
        }
        result.copy_from_slice(&bytes);
        Ok(Self {
            key: result
        })
    }
}

impl ServerPublicKey {
    pub fn get_host(&self) -> String {
        let checksum = &[PREFIX.as_bytes(), self.key.as_ref()].concat();
        let checksum = digest::digest(&digest::SHA256, checksum);
        println!("checksum: {:?}", checksum);
        let checksum = base_x::encode(BASE36_ENCODER_STRING, checksum.as_ref());
        checksum[0..HOSTNAME_LENGTH].to_string()
    }
    pub fn create_challange(&self) -> ChallengeDataType {
        let rng = rand::SystemRandom::new();
        let mut result = [0u8; 64];
        rng.fill(&mut result).unwrap();
        result
    }
    pub fn verify(&self, data: &ChallengeDataType, signature: &SignatureDataType) -> bool {
        let data = create_challenge(data);
        let key = signature::UnparsedPublicKey::new(&signature::ED25519, self.key.as_ref());
        key.verify(data.as_ref(), signature).is_ok()
    }
}

impl fmt::Display for ServerPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", base_x::encode(BASE36_ENCODER_STRING, self.key.as_ref()))
    }
}
#[cfg(test)]
mod tests {
    use crate::crypto::{ServerPrivateKey, BASE36_ENCODER_STRING, ServerPublicKey};

    #[test]
    fn test() {
        assert_eq!(BASE36_ENCODER_STRING.len(), 36);
        let private = ServerPrivateKey::default();
        assert_ne!(ServerPrivateKey::default().to_string(), private.to_string());
        let public = private.get_public_key();
        let private_string = private.to_string();
        let public_string = public.to_string();

        println!("private: {}", private_string);
        println!("public: {}", public_string);
    }
    #[test]
    fn test_signature() {
        let private = ServerPrivateKey::default();
        let public = private.get_public_key();
        let challenge = public.create_challange();
        let signature = private.sign(&challenge);
        assert!(public.verify(&challenge, &signature));
    }
    #[test]
    fn test_signature_invalid() {
        let private = ServerPrivateKey::default();
        let public = private.get_public_key();
        let challenge = public.create_challange();
        let mut signature = private.sign(&challenge);
        signature[0] = 1;
        assert!(!public.verify(&challenge, &signature));
        let other_private = ServerPrivateKey::default();
        let signature = other_private.sign(&challenge);
        assert!(!public.verify(&challenge, &signature));
    }
}

