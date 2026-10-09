use serde ::{Deserialize, Serialize};
pub type PetaId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PetalToCoreMessage {
    /// A message sent from a petal to the core.
    /// This message is used to request the core to perform an action.
    ResourceRequest{resource: String, request: Request},
    selfModificationRequest{new_petal_hash: String, signature: Vec<u8>},
    InterPetalData {target_petal_id: PetaId, payload: Vec<u8>},
    /// A message sent from a petal to the core.
    /// This message is used to notify the core of an event that has occurred in the petal.
}
#[derive(Debug, Clone, Serialize, Deserialize)
pub enum CoreToPetalMessage {
    ResourceGranted{handle: u64},
    ResourceDenied{reason: String},
    ModificationApproved{new_petal_id: PetaId},
    ModificationDenied{reason: String},
    IncomingData {source_petal_id: PetaId, payload: Vec<u8>},
}

pub struct CoreManager; {
    active_petals: std::collections::HashMap<PetaId, PetalState>,
    auth_public_key: [u8; 32],
}

impl CoreManager {
    pub fn new(auth_public_key: [u8; 32]) -> Self {
        Self {
            active_petals: std::collections::HashMap::new(),
            auth_public_key,
        }
    }

    pub fn handle_message(&mut self, petal_id: PetaId, message: PetalToCoreMessage) -> Option<CoreToPetalMessage> {
        match message {
            PetalToCoreMessage::ResourceRequest { resource, request } => {
                // Handle resource request logic here
                // For example, check if the resource is available and grant or deny access
                Some(CoreToPetalMessage::ResourceGranted { handle: 42 }) // Example response
            }
            PetalToCoreMessage::selfModificationRequest { new_petal_hash, signature } => {
                // Handle self-modification request logic here
                // Verify the signature and approve or deny the modification
                Some(CoreToPetalMessage::ModificationApproved { new_petal_id: 1 }) // Example response
            }
            PetalToCoreMessage::InterPetalData { target_petal_id, payload } => {
                // Handle inter-petal data transfer logic here
                // Forward the payload to the target petal if it exists
                None // Example response (no direct response needed)
            }
        }
    }
    fn verify_and_apply_modification(&mut self, petal_id: PetalId, hash: String, signature: Vec<u8>) -> CoreToPetalMessage {
        CoreToPetalMessage::ModificationDenied { reason: "Crypto verification pending implementation".into() }
    }

    
}