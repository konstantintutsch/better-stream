use iced::{Task, Element, Fill};
use iced::widget::{container, row, column, button, text};

use crate::stream::rtsp;

#[derive(Debug, Clone, Copy)]
pub enum Controls {
    Next,
    Previous
}

#[derive(Default)]
pub struct Player {
    client: rtsp::Client
}

impl Player {
    pub fn new(client: rtsp::Client) -> Self {
        Self {
            client: client
        }
    }

    pub fn boot() -> (Self, Task<Controls>) {
        (
            Player::new(rtsp::Client::new(vec![rtsp::Source{url: "".to_string(), username: None, password: None}])),
            Task::none()
        )
    }

    pub fn update(&mut self, message: Controls) {
        match message {
            Controls::Next => self.client.next(),
            Controls::Previous => self.client.previous(),
        }
    }

    pub fn view(&self) -> Element<'_, Controls> {
        container(
            column![
                text(&self.client.current().url),
                row![
                    button("Previous").on_press(Controls::Previous),
                    button("Next").on_press(Controls::Next)
                ]
                .spacing(10)
            ]
            .spacing(10)
        )
        .center(Fill)
        .into()
    }
}
