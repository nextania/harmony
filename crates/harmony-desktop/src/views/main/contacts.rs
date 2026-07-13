use std::{collections::HashMap, sync::Arc};

use harmony_api::{AddContactOutcome, ContactAction, RelationshipState};
use iced::Task;

use crate::{
    Message,
    api::{ApiClient, UserProfile},
    errors::RenderableError,
    views::main::{MainMessage, fetch_profiles_task},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactStatus {
    Established,
    PendingRemote,
    PendingLocal,
    None,
    Blocked,
}

impl From<&RelationshipState> for ContactStatus {
    fn from(r: &RelationshipState) -> Self {
        match r {
            RelationshipState::Established { .. } => ContactStatus::Established,
            RelationshipState::Blocked => ContactStatus::Blocked,
            RelationshipState::Requested { public_key: None } => ContactStatus::PendingRemote,
            RelationshipState::Requested { .. } => ContactStatus::PendingLocal,
            RelationshipState::PendingKeyExchange { .. } => ContactStatus::PendingRemote,
            RelationshipState::None => ContactStatus::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub user_id: String,
    pub status: ContactStatus,
}

impl Contact {
    pub fn from_outcome(outcome: AddContactOutcome) -> Self {
        match outcome {
            AddContactOutcome::Response(resp) => Contact {
                user_id: resp.profile.id,
                status: ContactStatus::from(&resp.state),
            },
            AddContactOutcome::Established { user_id } => Contact {
                user_id,
                status: ContactStatus::Established,
            },
        }
    }
}

#[derive(Clone)]
pub enum ContactsMessage {
    Loaded(Vec<Contact>, Vec<UserProfile>),
    AddInputChanged(String),
    AddSubmit,
    Added(Contact, Option<UserProfile>),
    Remove(String),
    Removed(String),
    Accept(String),
    Accepted(Contact),
    Block(String),
    Blocked(String),
    Unblock(String),
    Unblocked(Contact),
}

fn msg(m: ContactsMessage) -> Message {
    Message::Main(MainMessage::Contacts(m))
}

fn err(e: RenderableError) -> Message {
    Message::Main(MainMessage::ApiError(e))
}

#[derive(Default)]
pub struct ContactsState {
    pub list: Vec<Contact>,
    pub loaded: bool,
    pub add_input: String,
}

impl ContactsState {
    pub fn update(
        &mut self,
        message: ContactsMessage,
        api: &Arc<ApiClient>,
        profiles: &mut HashMap<String, UserProfile>,
    ) -> Task<Message> {
        match message {
            ContactsMessage::Loaded(contacts, loaded_profiles) => {
                profiles.extend(loaded_profiles.into_iter().map(|p| (p.id.clone(), p)));
                self.list = contacts;
                self.loaded = true;
            }
            ContactsMessage::AddInputChanged(s) => self.add_input = s,
            ContactsMessage::AddSubmit => {
                let username = self.add_input.trim().to_string();
                if !username.is_empty() {
                    self.add_input.clear();
                    let client = api.clone();
                    return Task::perform(
                        async move {
                            let profile = client.get_profile_by_username(&username).await?;
                            let outcome = client
                                .add_contact(ContactAction::Request {
                                    user_id: profile.id.clone(),
                                })
                                .await?;
                            Ok((Contact::from_outcome(outcome), profile))
                        },
                        |result: crate::errors::RenderableResult<_>| match result {
                            Ok((contact, profile)) => {
                                msg(ContactsMessage::Added(contact, Some(profile)))
                            }
                            Err(e) => err(e),
                        },
                    );
                }
            }
            ContactsMessage::Added(contact, profile) => {
                if let Some(profile) = profile {
                    profiles.insert(profile.id.clone(), profile);
                }
                if !self.list.iter().any(|c| c.user_id == contact.user_id) {
                    self.list.push(contact);
                }
            }
            ContactsMessage::Remove(user_id) => {
                let client = api.clone();
                let uid = user_id.clone();
                return Task::perform(
                    async move {
                        client.client().remove_contact(&uid).await?;
                        Ok::<(), RenderableError>(())
                    },
                    move |result| match result {
                        Ok(()) => msg(ContactsMessage::Removed(user_id.clone())),
                        Err(e) => err(e),
                    },
                );
            }
            ContactsMessage::Removed(user_id) => {
                self.list.retain(|c| c.user_id != user_id);
            }
            ContactsMessage::Accept(user_id) => {
                let client = api.clone();
                return Task::perform(
                    async move { client.add_contact(ContactAction::Accept { user_id }).await },
                    |result| match result {
                        Ok(outcome) => {
                            msg(ContactsMessage::Accepted(Contact::from_outcome(outcome)))
                        }
                        Err(e) => err(e),
                    },
                );
            }
            ContactsMessage::Accepted(contact) => {
                if let Some(c) = self.list.iter_mut().find(|c| c.user_id == contact.user_id) {
                    c.status = contact.status;
                }
            }
            ContactsMessage::Block(user_id) => {
                let client = api.clone();
                let uid = user_id.clone();
                return Task::perform(
                    async move {
                        client.client().block_contact(&uid).await?;
                        Ok::<(), RenderableError>(())
                    },
                    move |result| match result {
                        Ok(()) => msg(ContactsMessage::Blocked(user_id.clone())),
                        Err(e) => err(e),
                    },
                );
            }
            ContactsMessage::Blocked(user_id) => {
                if let Some(c) = self.list.iter_mut().find(|c| c.user_id == user_id) {
                    c.status = ContactStatus::Blocked;
                }
            }
            ContactsMessage::Unblock(user_id) => {
                let client = api.clone();
                let uid = user_id.clone();
                return Task::perform(
                    async move {
                        let c = client.client().unblock_contact(&uid).await?;
                        Ok::<Contact, RenderableError>(Contact {
                            user_id: c.id,
                            status: ContactStatus::from(&c.state),
                        })
                    },
                    move |result| match result {
                        Ok(contact) => msg(ContactsMessage::Unblocked(contact)),
                        Err(e) => err(e),
                    },
                );
            }
            ContactsMessage::Unblocked(contact) => {
                if let Some(c) = self.list.iter_mut().find(|c| c.user_id == contact.user_id) {
                    c.status = ContactStatus::Established;
                }
            }
        }
        Task::none()
    }

    pub fn load_task(api: &Arc<ApiClient>) -> Task<Message> {
        let client = api.clone();
        Task::perform(
            async move {
                let contacts = client.client().get_contacts().await?;
                let ids: Vec<String> = contacts.iter().map(|c| c.id.clone()).collect();
                let profiles = client.get_profiles(ids).await.unwrap_or_default();
                let contacts = contacts
                    .into_iter()
                    .map(|c| Contact {
                        user_id: c.id,
                        status: ContactStatus::from(&c.state),
                    })
                    .collect();
                Ok::<_, RenderableError>((contacts, profiles))
            },
            |result| match result {
                Ok((contacts, profiles)) => msg(ContactsMessage::Loaded(contacts, profiles)),
                Err(e) => err(e),
            },
        )
    }

    pub fn on_state_changed(
        &mut self,
        user_id: String,
        state: &RelationshipState,
        api: &Arc<ApiClient>,
    ) -> Task<Message> {
        if matches!(state, RelationshipState::None) {
            self.list.retain(|c| c.user_id != user_id);
        } else if !matches!(
            state,
            RelationshipState::PendingKeyExchange { .. } | RelationshipState::Established { .. }
        ) {
            let new_status = ContactStatus::from(state);
            if let Some(c) = self.list.iter_mut().find(|c| c.user_id == user_id) {
                c.status = new_status;
            } else {
                self.list.push(Contact {
                    user_id: user_id.clone(),
                    status: new_status,
                });
                return fetch_profiles_task(api.clone(), vec![user_id]);
            }
        }
        Task::none()
    }
}
