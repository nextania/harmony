use futures_util::StreamExt;
use harmony_types::users::UserProfile;
use mongodb::bson::{self, doc};
use serde::{Deserialize, Serialize};

use super::channels::Channel;
use crate::{errors::{Error, Result}, services::redis::is_user_online};

pub use harmony_types::users::{Contact, ContactExtended, Presence, Relationship, Status};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeyPackage {
    // x25519 public key for persistent encryption (DMs / persistent group channels)
    pub public_key: Vec<u8>,
    // encrypted private key and other key material, encrypted by a key derived from the user's password
    pub encrypted_keys: Vec<u8>,
}

pub async fn get_presentable_presence(user: &User) -> Result<Presence> {
    let user_online = is_user_online(&user.id).await?;
    Ok(if user_online && !matches!(user.presence.status, Status::Offline) {
        user.presence.clone()
    } else {
        Presence {
            status: Status::Offline,
            message: String::new(),
        }
    })
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

    pub async fn add_contact(&self, contact_id: &String) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        User::get(contact_id).await?;
        let contact = self.contacts.iter().find(|a| &a.id == contact_id);
        if let Some(contact) = contact {
            match contact.relationship {
                Relationship::Established => Err(Error::AlreadyEstablished),
                Relationship::Blocked => Err(Error::Blocked),
                Relationship::Requested => Err(Error::AlreadyRequested),
                Relationship::Pending => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$set": {
                                    "contacts.$[contact].relationship": bson::to_bson(&Relationship::Established)?
                                }
                            }).with_options(
                            Some(mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! {
                                    "contact.id": &contact_id
                                }])
                                .build()),
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": &contact_id
                            },
                            doc! {
                                "$set": {
                                    "contacts.$[contact].relationship": bson::to_bson(&Relationship::Established)?
                                }
                            }).with_options(
                            Some(mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! {
                                    "contact.id": &self.id
                                }])
                                .build()),
                        )
                        .await?;
                    Ok(())
                }
            }
        } else {
            users
                .update_one(
                    doc! {
                        "id": &self.id
                    },
                    doc! {
                        "$push": {
                            "contacts": {
                                "id": contact_id,
                                "relationship": bson::to_bson(&Relationship::Requested)?
                            }
                        }
                    },
                )
                .await?;
            users
                .update_one(
                    doc! {
                        "id": &contact_id
                    },
                    doc! {
                        "$push": {
                            "contacts": {
                                "id": &self.id,
                                "relationship": bson::to_bson(&Relationship::Pending)?
                            }
                        }
                    },
                )
                .await?;
            Ok(())
        }
    }

    pub async fn remove_contact(&self, contact_id: &String) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        User::get(contact_id).await?;
        let contact = self.contacts.iter().find(|a| &a.id == contact_id);
        if let Some(contact) = contact {
            match contact.relationship {
                // remove contact
                Relationship::Established => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": contact_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": contact_id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": &self.id
                                    }
                                }
                            },
                        )
                        .await?;
                    Ok(())
                }
                Relationship::Blocked => Err(Error::Blocked),
                // revoke friend request
                Relationship::Requested => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": contact_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": contact_id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": &self.id
                                    }
                                }
                            },
                        )
                        .await?;
                    Ok(())
                }
                // deny friend request
                Relationship::Pending => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": contact_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": contact_id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": &self.id
                                    }
                                }
                            },
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
            if contact.relationship == Relationship::Established {
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
                relationship: contact.relationship.clone(),
                user: UserProfile {
                    id: user.id.clone(),
                    public_key: user.key_package.as_ref().map(|kp| kp.public_key.clone()),
                    presence: if contact.relationship == Relationship::Established {
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

    pub async fn relationship_with(&self, other_id: &String) -> Result<Option<Relationship>> {
        let contact = self.contacts.iter().find(|c| &c.id == other_id);
        Ok(contact.map(|c| c.relationship.clone()))
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

    pub async fn set_key_package(
        &self,
        public_key: Vec<u8>,
        encrypted_keys: Vec<u8>,
    ) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        users
            .update_one(
                doc! { "id": &self.id },
                doc! {
                    "$set": {
                        "keyPackage": bson::to_bson(&KeyPackage {
                            public_key,
                            encrypted_keys,
                        })?
                    }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn can_dm(&self, other: &User) -> Result<bool> {
        let contact = self.contacts.iter().find(|c| c.id == other.id);
        if let Some(contact) = contact {
            Ok(contact.relationship == Relationship::Established)
        } else {
            Ok(false)
        }
    }
}
