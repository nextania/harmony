use std::sync::Arc;

use async_notify::Notify;

#[derive(Clone, Debug)]
pub struct Request<T: Clone> {
    data: Option<T>,
    notify: Arc<Notify>
}

impl<T: Clone> Request<T> {
    pub fn new() -> Self {
        Self {
            data: None,
            notify: Arc::new(Notify::new())
        }
    }

    pub fn set(&mut self, data: T) {
        self.data = Some(data);
        self.notify.notify();
    }

    // TODO: timeout error
    pub async fn wait(&self) -> T {
        self.notify.notified().await;
        self.data.clone().unwrap()
    }
}