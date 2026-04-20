use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use sha2::{Sha256, Digest};

use crate::types::*;


#[derive(Clone)]
pub struct AuthKeys {
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
}

impl AuthKeys {
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        Self { signing_key, verifying_key }
    }
}

impl Message {
    pub fn sign(&mut self, signing_key: &SigningKey) -> anyhow::Result<()> {  
    	// Enlève la signature du message
        let mut headers_without_sig = self.headers.clone();
        headers_without_sig.signature = vec![];

        // Converti le message en bits
        let headers_bytes = bincode::serialize(&headers_without_sig)?;
        let payload_bytes = bincode::serialize(&self.payload)?;
        let mut to_sign = headers_bytes;
        to_sign.extend_from_slice(&payload_bytes);

        // Signe le message
        let signature = signing_key.sign(&to_sign);
        self.headers.signature = signature.to_bytes().to_vec();
        Ok(())
    }

    pub fn verify(&self, verifying_key: &VerifyingKey) -> bool {
    	// Extrait la signature et cloner le message sans la signature
	    let mut headers_without_sig = self.headers.clone();
	    let sig_bytes = headers_without_sig.signature.clone();
	    headers_without_sig.signature = vec![];

	    // Converti le message en bits, pour prochainement appliquer 'verify'
	    let Ok(headers_bytes) = bincode::serialize(&headers_without_sig) else { return false };
	    let Ok(payload_bytes) = bincode::serialize(&self.payload) else { return false };

	    let mut to_verify = headers_bytes;
	    to_verify.extend_from_slice(&payload_bytes);

	    // Convertir la signature extraite en bits
	    let Ok(sig_bytes_fixed) = <[u8; 64]>::try_from(sig_bytes.as_slice()) else { return false };
		let signature = Signature::from_bytes(&sig_bytes_fixed);

	    // Vérifier
	    verifying_key.verify(&to_verify, &signature).is_ok()
	}
}

pub fn peer_id_from_verifying_key(verifying_key: &VerifyingKey) -> PeerId {
	hex::encode(Sha256::digest(verifying_key.to_bytes()))
}

pub async fn get_verifying_key(peers: PeersMap, username: String) -> Option<VerifyingKey> {
	let peers = peers.lock().await;
	peers.values()
		.find(|peer_info| peer_info.username == username)
		.map(|peer_info| peer_info.verifying_key)
}
