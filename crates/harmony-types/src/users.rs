use serde::{Deserialize, Serialize};


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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Relationship {
    Established = 0,
    Blocked = 1,
    Requested = 2,
    Pending = 3,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Contact {
    pub id: String,
    pub relationship: Relationship,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContactExtended {
    pub id: String,
    pub relationship: Relationship,
    pub user: UserProfile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    /// x25519 public key (persistent encryption). `None` if no keys uploaded yet.
    pub public_key: Option<Vec<u8>>,
    // None if user does not have established relationship with requester
    // or if this user data was sent in a context where presence is not relevant (e.g. message)
    pub presence: Option<Presence>,
}


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUserResponse {
    pub id: String,
    pub public_key: Option<Vec<u8>>,
    pub encrypted_keys: Option<Vec<u8>>,
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
    /// x25519 public key for persistent encryption (DMs / persistent group channels).
    pub public_key: Vec<u8>,
    /// Encrypted private key material (encrypted client-side, opaque to server).
    pub encrypted_keys: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyPackageResponse {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddContactMethod {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AddContactResponse {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddContactUsernameMethod {
    pub username: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AddContactUsernameResponse {}

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