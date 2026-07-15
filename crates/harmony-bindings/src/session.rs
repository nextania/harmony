use std::sync::Arc;

use crate::error::HarmonyResult;

#[derive(uniffi::Object)]
pub struct Session {
    pub(crate) inner: Arc<core_api::Session>,
}

#[derive(uniffi::Object)]
pub struct MfaLogin {
    inner: core_api::LoginMfa,
}

#[uniffi::export]
impl MfaLogin {
    pub async fn code(&self, code: String) -> HarmonyResult<Arc<Session>> {
        let session = self.inner.code(&code).await?;
        Ok(Arc::new(Session {
            inner: Arc::new(session),
        }))
    }
}

#[derive(uniffi::Object)]
pub struct LoginOutcome {
    session: Option<Arc<Session>>,
    mfa: Option<Arc<MfaLogin>>,
}

#[uniffi::export]
impl LoginOutcome {
    pub fn session(&self) -> Option<Arc<Session>> {
        self.session.clone()
    }

    pub fn mfa(&self) -> Option<Arc<MfaLogin>> {
        self.mfa.clone()
    }
}

#[uniffi::export]
pub async fn login(
    account_url: String,
    email: String,
    password: String,
) -> HarmonyResult<Arc<LoginOutcome>> {
    let outcome = match core_api::login(&account_url, &email, &password).await? {
        core_api::LoginResult::Success(session) => LoginOutcome {
            session: Some(Arc::new(Session {
                inner: Arc::new(session),
            })),
            mfa: None,
        },
        core_api::LoginResult::RequiresContinuation(mfa) => LoginOutcome {
            session: None,
            mfa: Some(Arc::new(MfaLogin { inner: mfa })),
        },
    };
    Ok(Arc::new(outcome))
}
