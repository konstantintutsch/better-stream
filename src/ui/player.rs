use iced::{Element, Fill};
use iced::widget::{container, row, column, button, text};

use crate::stream::rtsp;

#[derive(Debug, Clone, Copy)]
pub enum Controls {
    Next,
    Previous
}

#[derive(Default)]
pub struct Interface {
    source: rtsp::Source
}

impl Interface {
    pub fn update(&mut self, message: Controls) {
        self.source.url = match message {
            Controls::Next => "👉".to_string(),
            Controls::Previous => "👈".to_string()
        }
    }

    pub fn view(&self) -> Element<'_, Controls> {
        container(
            column![
                text(&self.source.url),
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
