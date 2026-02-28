use std::sync::Arc;

use async_trait::async_trait;
use iced::color;
use ulid::Ulid;

use crate::{
    MessageAuthor,
    api::{
        ApiClient, ApiMessage, ApiMessageContent, CallParticipant, CallState, CallTrackState,
        Channel, Contact, ContactStatus, CurrentUser, UserProfile, UserStatus,
    },
    errors::{RenderableError, RenderableResult},
};

#[derive(Debug, Clone)]
pub struct MockApiClient;

impl MockApiClient {
    pub const ALICE_ID: &'static str = "01KJ2XAVGXTH61W4RK17N47M2X";
    pub const BOB_ID: &'static str = "01KJ2XC6GZ5VBT0JQEKVSN5ZBZ";
    pub const CHARLIE_ID: &'static str = "01KJ2XCN3TGQ7EBEZK9YKPWR4E";
    pub const ME_ID: &'static str = "01KJ2XCRE6K89015WYTMDX1XTK";

    pub fn with_credentials(
        email: &str,
        password: &str,
    ) -> Result<Arc<dyn ApiClient>, RenderableError> {
        if email.is_empty() || password.is_empty() {
            Err(RenderableError::IncorrectCredentials)
        } else {
            Ok(Arc::new(Self))
        }
    }
}

#[async_trait]
impl ApiClient for MockApiClient {
    async fn get_current_user(&self) -> Result<CurrentUser, RenderableError> {
        Ok(CurrentUser {
            profile: UserProfile {
                id: Self::ME_ID.into(),
                display_name: "User".into(),
                username: "username".into(),
                avatar_color_start: color!(0xff5b5b),
                avatar_color_end: color!(0xfe44b4),
            },
            status: UserStatus::Online,
            email: "user@example.com".into(),
        })
    }

    async fn get_conversations(&self) -> RenderableResult<Vec<Channel>> {
        let alice = UserProfile {
            id: Self::ALICE_ID.into(),
            display_name: "Alice".into(),
            username: "alice".into(),
            avatar_color_start: color!(0x00b536),
            avatar_color_end: color!(0xffce2c),
        };
        let bob = UserProfile {
            id: Self::BOB_ID.into(),
            display_name: "Bob".into(),
            username: "bob".into(),
            avatar_color_start: color!(0x06b2c1),
            avatar_color_end: color!(0xaa2cff),
        };
        let charlie = UserProfile {
            id: Self::CHARLIE_ID.into(),
            display_name: "Charlie".into(),
            username: "charlie".into(),
            avatar_color_start: color!(0x8b00ae),
            avatar_color_end: color!(0x4400ae),
        };

        Ok(vec![
            Channel::Private {
                id: Self::ALICE_ID.into(),
                other: alice.clone(),
            },
            Channel::Private {
                id: Self::BOB_ID.into(),
                other: bob.clone(),
            },
            Channel::Private {
                id: Self::CHARLIE_ID.into(),
                other: charlie.clone(),
            },
        ])
    }

    async fn get_messages(&self, id: String) -> RenderableResult<Vec<ApiMessage>> {
        // simulate network delay
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let alice_author = MessageAuthor::User {
            id: Self::ALICE_ID.into(),
            name: "Alice".into(),
            avatar_color_start: color!(0x00b536),
            avatar_color_end: color!(0xffce2c),
        };
        let bob_author = MessageAuthor::User {
            id: Self::BOB_ID.into(),
            name: "Bob".into(),
            avatar_color_start: color!(0x06b2c1),
            avatar_color_end: color!(0xaa2cff),
        };
        let charlie_author = MessageAuthor::User {
            id: Self::CHARLIE_ID.into(),
            name: "Charlie".into(),
            avatar_color_start: color!(0x8b00ae),
            avatar_color_end: color!(0x4400ae),
        };
        let me_author = MessageAuthor::User {
            id: Self::ME_ID.into(),
            name: "User".into(),
            avatar_color_start: color!(0xff5b5b),
            avatar_color_end: color!(0xfe44b4),
        };
        // Err(RenderableError::IncorrectCredentials)
        Ok(match id.as_str() {
            Self::ALICE_ID => vec![
                ApiMessage {
                    id: Ulid::from_parts(0, 1).to_string(),
                    author: alice_author.clone(),
                    content: ApiMessageContent::Text("Hey, how are you? 😊".into()),
                },
                ApiMessage {
                    id: Ulid::from_parts(0, 2).to_string(),
                    author: me_author.clone(),
                    content: ApiMessageContent::Text("I'm doing great, thanks!".into()),
                },
                ApiMessage {
                    id: Ulid::from_parts(0, 3).to_string(),
                    author: alice_author.clone(),
                    content: ApiMessageContent::CallCard {
                        channel: "General".into(),
                        duration: "00:01:07".into(),
                    },
                },
                ApiMessage {
                    id: Ulid::from_parts(0, 4).to_string(),
                    author: me_author.clone(),
                    content: ApiMessageContent::Text(
                        "Let me know when you want to catch up again!".into(),
                    ),
                },
            ],
            Self::BOB_ID => vec![
                ApiMessage {
                    id: Ulid::from_parts(0, 5).to_string(),
                    author: bob_author.clone(),
                    content: ApiMessageContent::Text("Hey, did you see the latest update?".into()),
                },
                ApiMessage {
                    id: Ulid::from_parts(0, 6).to_string(),
                    author: me_author.clone(),
                    content: ApiMessageContent::Text("Not yet, what changed?".into()),
                },
                ApiMessage {
                    id: Ulid::from_parts(0, 7).to_string(),
                    author: bob_author.clone(),
                    content: ApiMessageContent::Text(
                        "They shipped voice channels and the new search UI 🎉".into(),
                    ),
                },
            ],
            Self::CHARLIE_ID => vec![ApiMessage {
                id: Ulid::from_parts(0, 8).to_string(),
                author: charlie_author.clone(),
                content: ApiMessageContent::Text("Good morning!".into()),
            }],
            _ => vec![],
        })
    }

    async fn send_message(
        &self,
        _channel_id: String,
        content: String,
    ) -> RenderableResult<ApiMessage> {
        let me_author = MessageAuthor::User {
            id: Self::ME_ID.into(),
            name: "User".into(),
            avatar_color_start: color!(0xff5b5b),
            avatar_color_end: color!(0xfe44b4),
        };
        Ok(ApiMessage {
            id: Ulid::new().to_string(),
            author: me_author,
            content: ApiMessageContent::Text(content),
        })
    }

    async fn edit_message(
        &self,
        message_id: String,
        _channel_id: String,
        content: String,
    ) -> RenderableResult<ApiMessage> {
        let me_author = MessageAuthor::User {
            id: Self::ME_ID.into(),
            name: "User".into(),
            avatar_color_start: color!(0xff5b5b),
            avatar_color_end: color!(0xfe44b4),
        };
        Ok(ApiMessage {
            id: message_id,
            author: me_author,
            content: ApiMessageContent::Text(content),
        })
    }

    async fn delete_message(&self, _message_id: String) -> RenderableResult<()> {
        Ok(())
    }

    async fn get_call(&self, channel_id: String) -> RenderableResult<Option<CallState>> {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(match channel_id.as_str() {
            Self::ALICE_ID => Some(CallState {
                participants: vec![CallParticipant {
                    profile: UserProfile {
                        id: Self::ALICE_ID.into(),
                        display_name: "Alice".into(),
                        username: "alice".into(),
                        avatar_color_start: color!(0x00b536),
                        avatar_color_end: color!(0xffce2c),
                    },
                    tracks: CallTrackState {
                        audio: true,
                        video: false,
                        screen: false,
                    },
                }],
            }),
            _ => None,
        })
    }

    async fn get_contacts(&self) -> RenderableResult<Vec<Contact>> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(vec![
            Contact {
                profile: UserProfile {
                    id: Self::ALICE_ID.into(),
                    display_name: "Alice".into(),
                    username: "alice".into(),
                    avatar_color_start: color!(0x00b536),
                    avatar_color_end: color!(0xffce2c),
                },
                status: ContactStatus::Established,
            },
            Contact {
                profile: UserProfile {
                    id: Self::BOB_ID.into(),
                    display_name: "Bob".into(),
                    username: "bob".into(),
                    avatar_color_start: color!(0x06b2c1),
                    avatar_color_end: color!(0xaa2cff),
                },
                status: ContactStatus::Pending,
            },
            Contact {
                profile: UserProfile {
                    id: Self::CHARLIE_ID.into(),
                    display_name: "Charlie".into(),
                    username: "charlie".into(),
                    avatar_color_start: color!(0x8b00ae),
                    avatar_color_end: color!(0x4400ae),
                },
                status: ContactStatus::Requested,
            },
        ])
    }

    async fn add_contact(&self, username: String) -> RenderableResult<Contact> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(Contact {
            profile: UserProfile {
                id: Ulid::new().to_string(),
                display_name: username.clone(),
                username,
                avatar_color_start: color!(0x555555),
                avatar_color_end: color!(0x888888),
            },
            status: ContactStatus::Pending,
        })
    }

    async fn remove_contact(&self, _user_id: String) -> RenderableResult<()> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(())
    }

    async fn accept_contact(&self, user_id: String) -> RenderableResult<Contact> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(Contact {
            profile: crate::api::placeholder_profile(&user_id),
            status: ContactStatus::Established,
        })
    }

    async fn block_contact(&self, _user_id: String) -> RenderableResult<()> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(())
    }

    async fn unblock_contact(&self, user_id: String) -> RenderableResult<Contact> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(Contact {
            profile: crate::api::placeholder_profile(&user_id),
            status: ContactStatus::Established,
        })
    }
}
