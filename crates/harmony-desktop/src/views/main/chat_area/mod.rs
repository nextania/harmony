use iced::{Element, Length, widget::column};

use crate::views::main::{
    ChatMode, MainMessage, MainView,
    chat_area::{input::chat_frame, messages::main_chat, voice::voice_area},
};

pub mod input;
pub mod messages;
pub mod top_bar;
pub mod voice;

pub fn chat_area(state: &MainView) -> Element<MainMessage> {
    let top_bar = top_bar::top_bar(state);
    let body: Element<MainMessage> = match state.chat_mode {
        ChatMode::Voice => voice_area(state),
        ChatMode::Text => {
            let main_chat = main_chat(state);
            let chat_frame = chat_frame(state);
            column![main_chat, chat_frame]
                .width(Length::Fill)
                .height(Length::Fill)
                .spacing(0)
                .into()
        }
    };
    column![top_bar, body]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(0)
        .into()
}
