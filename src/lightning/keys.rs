use std::fs; //intéraction avec le système de fichiers
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use lightning::bitcoin::secp256k1::PublicKey;
use lightning::sign::KeysManager;
use lightning::sign::NodeSigner;

#[derive(Clone)]
pub struct LdkKeysManager {
    //Warper De KeysManager de LDK dans un Arc (multi thread-safe)
    pub inner: Arc<KeysManager>,
}

impl LdkKeysManager {

    //Charge une grainde depuis un fichier pour l'initialisation
    //Lit les 32 bytes du fichier identity.key (ed25519) et les utilise comme seed
    pub fn load(path: &str) -> anyhow::Result<Self> {
        // Lire les 32 bytes depuis le fichier hex puis on s'assure qu'elle a la taille standard (32 octets) 
        let hex = fs::read_to_string(path)?;
        let bytes = hex::decode(hex.trim())?;
        let seed: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("identity.key doit faire exactement 32 bytes"))?;

        //On va utiliser le temps comme source de hasard pour différentier les sessions
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        
        //Clé de session temporaire qui change à chaque session
        let keys_manager = KeysManager::new(
            &seed,
            now.as_secs(),
            now.subsec_nanos(),
        );

        Ok(Self {
            inner: Arc::new(keys_manager),
        })
    }

    /// Retourne la clé publique secp256k1 du "node_id" LN
    /// Sorte de carte d'identié publique sur le réseau LN
    /// Différent de la verifying_key ed25519 
    pub fn node_id(&self) -> PublicKey {
        self.inner.get_node_id(
            lightning::sign::Recipient::Node,
        )
        .expect("KeysManager::get_node_id ne peut pas échouer pour Recipient::Node")
    }

    /// Retourne le node_id formaté en hex (format standard LN)
    pub fn node_id_hex(&self) -> String {
        hex::encode(self.node_id().serialize())
    }
}