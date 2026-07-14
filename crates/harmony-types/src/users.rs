use serde::{Deserialize, Serialize};
use serde_with::serde_as;

pub const MLKEM768_EK_BYTES: usize = 1184;
pub const MLKEM768_CT_BYTES: usize = 1088;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Status {
    Online = 0,
    Idle = 1,
    Busy = 2,
    BusyNotify = 3,
    Offline = 4,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Presence {
    pub status: Status,
    pub message: String,
}

/// Combined X25519 + ML-KEM-768 public key for hybrid post-quantum key exchange.
/// Layout: [ x25519_pk (32 bytes) | mlkem_ek (1184 bytes) ]
#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HybridPublicKey {
    pub x25519: [u8; 32],
    #[serde_as(as = "Box<[_; 1184]>")]
    pub mlkem: Box<[u8; 1184]>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedPublicKey {
    pub hybrid: HybridPublicKey,
    pub ed25519: [u8; 32],
}

/// Raw ML-KEM-768 ciphertext (1088 bytes) produced during encapsulation.
pub type Encapsulated = Box<[u8; 1088]>;

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum RelationshipState {
    None,
    Requested {
        public_key: Option<UnifiedPublicKey>,
    },
    PendingKeyExchange {
        public_key: Option<UnifiedPublicKey>,
        #[serde_as(as = "Option<Box<[_; 1088]>>")]
        encapsulated: Option<Encapsulated>,
    },
    Established {
        public_key: UnifiedPublicKey,
        #[serde_as(as = "Box<[_; 1088]>")]
        encapsulated: Encapsulated,
        key_id: String,
    },
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Contact {
    pub id: String,
    pub state: RelationshipState,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContactExtended {
    pub id: String,
    pub state: RelationshipState,
    pub user: UserProfile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    // None if user does not have established relationship with requester
    // or if this user data was sent in a context where presence is not relevant (e.g. message)
    pub presence: Option<Presence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUserResponse {
    pub id: String,
    pub encrypted_keys: Option<Vec<u8>>,
    pub keystore_generation: u64,
    pub presence: Presence,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCurrentUserMethod {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetUserMethod {
    pub user_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetUserResponse {
    pub user: UserProfile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyPackageMethod {
    pub encrypted_keys: Vec<u8>,
    pub expected_generation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyPackageResponse {
    pub generation: u64,
}

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum AddContactStage {
    // 1. send a request with our public key
    Request {
        id: String,
        public_key: UnifiedPublicKey,
    },
    // 2. accept the request and send our public key + ML-KEM encapsulation to requester
    Accept {
        user_id: String,
        public_key: UnifiedPublicKey,
        #[serde_as(as = "Box<[_; 1088]>")]
        encapsulated: Encapsulated,
    },
    // 3. finalize and send our ML-KEM encapsulation back to the acceptor
    Finalize {
        user_id: String,
        public_key: UnifiedPublicKey,
        #[serde_as(as = "Box<[_; 1088]>")]
        encapsulated: Encapsulated,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddContactMethod {
    pub stage: AddContactStage,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AddContactResponse {
    pub profile: UserProfile,
    pub state: RelationshipState,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveContactMethod {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveContactResponse {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetContactsMethod {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetContactsResponse {
    pub contacts: Vec<ContactExtended>,
}
// FIXME: implement these methods on server
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockContactMethod {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BlockContactResponse {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnblockContactMethod {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnblockContactResponse {
    pub contact: ContactExtended,
}
