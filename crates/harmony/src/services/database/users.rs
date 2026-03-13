use futures_util::StreamExt;
use harmony_types::users::{AddContactStage, UserProfile};
use mongodb::{
    bson::{self, doc},
    options::UpdateOptions,
};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::channels::Channel;
use crate::{
    errors::{Error, Result},
    services::redis::is_user_online,
};

pub use harmony_types::users::{
    Contact, ContactExtended, Encapsulated, Presence, RelationshipState, Status, UnifiedPublicKey,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeyPackage {
    // encrypted local keystore blob (encrypted by a key derived from the user's password)
    pub encrypted_keys: Vec<u8>,
}

pub async fn get_presentable_presence(user: &User) -> Result<Presence> {
    let user_online = is_user_online(&user.id).await?;
    Ok(
        if user_online && !matches!(user.presence.status, Status::Offline) {
            user.presence.clone()
        } else {
            Presence {
                status: Status::Offline,
                message: String::new(),
            }
        },
    )
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    // pub profile_banner: Option<String>, // TODO: move to AS
    pub contacts: Vec<Contact>,
    pub key_package: Option<KeyPackage>,
    pub presence: Presence,
}

impl User {
    pub async fn in_channel(&self, channel: &Channel) -> Result<bool> {
        match channel {
            Channel::PrivateChannel {
                initiator_id,
                target_id,
                ..
            } => {
                if initiator_id == &self.id || target_id == &self.id {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Channel::GroupChannel { members, .. } => {
                Ok(members.iter().any(|member| member.id == self.id))
            }
        }
    }

    pub async fn get(id: &String) -> Result<User> {
        let users = super::get_database().collection::<User>("users");
        let user = users
            .find_one(doc! {
                "id": id
            })
            .await?;
        match user {
            Some(user) => Ok(user),
            None => Err(Error::NotFound),
        }
    }
    pub async fn get_by_username(username: &String) -> Result<User> {
        let users = super::get_database().collection::<User>("users");
        let user = users
            .find_one(doc! {
                "username": username
            })
            .await?;
        match user {
            Some(user) => Ok(user),
            None => Err(Error::NotFound),
        }
    }

    pub async fn create(id: String) -> Result<User> {
        let users = super::get_database().collection::<User>("users");
        let user = User {
            id,
            contacts: Vec::new(),
            key_package: None,
            presence: Presence {
                status: Status::Online,
                message: String::new(),
            },
        };
        users.insert_one(user.clone()).await?;
        Ok(user)
    }

    pub async fn add_contact(
        &self,
        stage: AddContactStage,
    ) -> Result<(UserProfile, RelationshipState)> {
        let users = super::get_database().collection::<User>("users");
        match stage {
            AddContactStage::Request {
                username,
                public_key,
            } => {
                let target = User::get_by_username(&username).await?;
                let contact_id = &target.id;
                let existing = self.contacts.iter().find(|a| &a.id == contact_id);
                if let Some(existing) = existing {
                    match &existing.state {
                        RelationshipState::Established { .. } => {
                            return Err(Error::AlreadyEstablished);
                        }
                        RelationshipState::Blocked => return Err(Error::Blocked),
                        RelationshipState::Requested { .. } => return Err(Error::AlreadyRequested),
                        RelationshipState::PendingKeyExchange { .. } => {
                            return Err(Error::AlreadyRequested);
                        }
                        RelationshipState::None => {} // allow re-request
                    }
                }
                let self_state = RelationshipState::Requested { public_key: None };
                let target_state = RelationshipState::Requested {
                    public_key: Some(public_key),
                };

                if existing.is_some() {
                    users
                        .update_one(
                            doc! { "id": &self.id },
                            doc! { "$set": { "contacts.$[contact].state": bson::to_bson(&self_state)? } },
                        )
                        .with_options(Some(
                            mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! { "contact.id": contact_id }])
                                .build(),
                        ))
                        .await?;
                } else {
                    users
                        .update_one(
                            doc! { "id": &self.id },
                            doc! { "$push": { "contacts": bson::to_bson(&Contact { id: contact_id.clone(), state: self_state.clone() })? } },
                        )
                        .await?;
                }

                let target_existing = target.contacts.iter().find(|a| a.id == self.id);
                if target_existing.is_some() {
                    users
                        .update_one(
                            doc! { "id": contact_id },
                            doc! { "$set": { "contacts.$[contact].state": bson::to_bson(&target_state)? } },
                        )
                        .with_options(Some(
                            mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! { "contact.id": &self.id }])
                                .build(),
                        ))
                        .await?;
                } else {
                    users
                        .update_one(
                            doc! { "id": contact_id },
                            doc! { "$push": { "contacts": bson::to_bson(&Contact { id: self.id.clone(), state: target_state })? } },
                        )
                        .await?;
                }

                Ok((
                    UserProfile {
                        id: target.id.clone(),
                        presence: None,
                    },
                    self_state,
                ))
            }
            AddContactStage::Accept {
                user_id,
                public_key,
                encapsulated,
            } => {
                let contact = self
                    .contacts
                    .iter()
                    .find(|a| a.id == user_id)
                    .ok_or(Error::NotFound)?;
                match &contact.state {
                    RelationshipState::Requested {
                        public_key: Some(_),
                    } => {}
                    _ => return Err(Error::InvalidStage),
                };

                let self_state = RelationshipState::PendingKeyExchange {
                    public_key: None,
                    encapsulated: None,
                };
                let requester_state = RelationshipState::PendingKeyExchange {
                    public_key: Some(public_key),
                    encapsulated: Some(encapsulated),
                };

                users
                    .update_one(
                        doc! { "id": &self.id },
                        doc! { "$set": { "contacts.$[contact].state": bson::to_bson(&self_state)? } },
                    )
                    .with_options(Some(
                        UpdateOptions::builder()
                            .array_filters(vec![doc! { "contact.id": &user_id }])
                            .build(),
                    ))
                    .await?;
                users
                    .update_one(
                        doc! { "id": &user_id },
                        doc! { "$set": { "contacts.$[contact].state": bson::to_bson(&requester_state)? } },
                    )
                    .with_options(Some(
                        UpdateOptions::builder()
                            .array_filters(vec![doc! { "contact.id": &self.id }])
                            .build(),
                    ))
                    .await?;

                Ok((
                    UserProfile {
                        id: user_id.clone(),
                        presence: None,
                    },
                    self_state,
                ))
            }
            AddContactStage::Finalize {
                user_id,
                public_key,
                encapsulated,
            } => {
                let contact = self
                    .contacts
                    .iter()
                    .find(|a| a.id == user_id)
                    .ok_or(Error::NotFound)?;
                let (peer_pk, their_ct) = match &contact.state {
                    RelationshipState::PendingKeyExchange {
                        public_key: Some(pk),
                        encapsulated: Some(ct),
                    } => (pk.clone(), ct.clone()),
                    _ => return Err(Error::InvalidStage),
                };
                let key_id = Ulid::new().to_string();

                let self_state = RelationshipState::Established {
                    public_key: peer_pk,
                    encapsulated: their_ct,
                    key_id: key_id.clone(),
                };
                let acceptor_state = RelationshipState::Established {
                    public_key,
                    encapsulated,
                    key_id: key_id.clone(),
                };

                users
                    .update_one(
                        doc! { "id": &self.id },
                        doc! { "$set": { "contacts.$[contact].state": bson::to_bson(&self_state)? } },
                    )
                    .with_options(Some(
                        UpdateOptions::builder()
                            .array_filters(vec![doc! { "contact.id": &user_id }])
                            .build(),
                    ))
                    .await?;
                users
                    .update_one(
                        doc! { "id": &user_id },
                        doc! { "$set": { "contacts.$[contact].state": bson::to_bson(&acceptor_state)? } },
                    )
                    .with_options(Some(
                        UpdateOptions::builder()
                            .array_filters(vec![doc! { "contact.id": &self.id }])
                            .build(),
                    ))
                    .await?;

                // if there is already a channel between the users,
                // this means that there was previously a relationship that was removed
                // then update the last_key_id for that channel with the new key_id
                let channel = Channel::get_between(&self.id, &user_id).await?;
                if let Some(channel) = channel {
                    channel.update_key_id(&key_id).await?;
                }

                Ok((
                    UserProfile {
                        id: user_id.clone(),
                        presence: Some(
                            get_presentable_presence(&User::get(&user_id).await?).await?,
                        ),
                    },
                    self_state,
                ))
            }
        }
    }

    pub async fn remove_contact(&self, contact_id: &String) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        User::get(contact_id).await?;
        let contact = self.contacts.iter().find(|a| &a.id == contact_id);
        if let Some(contact) = contact {
            match &contact.state {
                RelationshipState::Blocked => Err(Error::Blocked),
                _ => {
                    users
                        .update_one(
                            doc! { "id": &self.id },
                            doc! { "$pull": { "contacts": { "id": contact_id } } },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! { "id": contact_id },
                            doc! { "$pull": { "contacts": { "id": &self.id } } },
                        )
                        .await?;
                    Ok(())
                }
            }
        } else {
            Err(Error::NotFound)
        }
    }

    pub async fn get_established_contacts(&self) -> Result<Vec<User>> {
        let users = super::get_database().collection::<User>("users");
        let contacts = self.contacts.iter().map(|contact| async {
            if matches!(contact.state, RelationshipState::Established { .. }) {
                users
                    .find_one(doc! {
                        "id": &contact.id
                    })
                    .await
                    .ok()?
            } else {
                None
            }
        });
        let contacts: Vec<User> = futures_util::future::join_all(contacts)
            .await
            .iter()
            .filter_map(|contact| contact.clone())
            .collect();
        Ok(contacts)
    }

    pub async fn get_contacts(&self) -> Result<Vec<ContactExtended>> {
        let users = super::get_database().collection::<User>("users");
        let contacts = self.contacts.iter().map(|contact| async {
            let user = users
                .find_one(doc! {
                    "id": &contact.id
                })
                .await
                .ok()??;
            let presence = get_presentable_presence(&user).await.ok()?;
            Some(ContactExtended {
                id: contact.id.clone(),
                state: contact.state.clone(),
                user: UserProfile {
                    id: user.id.clone(),
                    presence: if matches!(contact.state, RelationshipState::Established { .. }) {
                        Some(presence)
                    } else {
                        None
                    },
                },
            })
        });
        let contacts: Vec<ContactExtended> = futures_util::future::join_all(contacts)
            .await
            .iter()
            .filter_map(|contact| contact.clone())
            .collect();
        Ok(contacts)
    }

    pub async fn relationship_with(&self, other_id: &String) -> Result<Option<RelationshipState>> {
        let contact = self.contacts.iter().find(|c| &c.id == other_id);
        Ok(contact.map(|c| c.state.clone()))
    }

    // pub async fn accept_invite(&self, invite_code: &String) -> Result<Space> {
    //     let invites = super::get_database().collection::<Invite>("invites");
    //     let spaces = super::get_database().collection::<Space>("spaces");
    //     let invite = invites
    //         .find_one_and_update(
    //             doc! {
    //                 "id": invite_code,
    //             },
    //             doc! {
    //                 "$push": {
    //                     "uses": &self.id,
    //                 }
    //             },
    //         )
    //         .await?;
    //     let invite = match invite {
    //         Some(invite) => invite,
    //         None => return Err(Error::NotFound),
    //     };
    //     let space = spaces
    //         .find_one(doc! {
    //             "id": invite.space_id,
    //         })
    //         .await?;
    //     let space = match space {
    //         Some(space) => space,
    //         None => return Err(Error::NotFound),
    //     };
    //     Ok(space)
    // }

    pub async fn get_channels(&self) -> Result<Vec<Channel>> {
        let channels = super::get_database().collection::<Channel>("channels");
        let channels = channels
            .find(doc! {
                "$or": [
                    {
                        "initiator_id": &self.id
                    },
                    {
                        "target_id": &self.id
                    },
                    {
                        "members": {
                            "$in": [&self.id],
                        }
                    }
                ]
            })
            .await?;
        let channels: Vec<Channel> = channels
            .filter_map(|channel| async { channel.ok() })
            .collect()
            .await;
        Ok(channels)
    }

    pub async fn set_key_package(&self, encrypted_keys: Vec<u8>) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        users
            .update_one(
                doc! { "id": &self.id },
                doc! {
                    "$set": {
                        "keyPackage": bson::to_bson(&KeyPackage {
                            encrypted_keys,
                        })?
                    }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn can_dm(&self, other: &User) -> Result<Option<String>> {
        let contact = self.contacts.iter().find(|c| c.id == other.id);
        if let Some(contact) = contact {
            match &contact.state {
                RelationshipState::Established { key_id, .. } => Ok(Some(key_id.clone())),
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}
