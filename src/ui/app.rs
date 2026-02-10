use iced::{
    Element, Fill,
    Task, Subscription, advanced::subscription,
    widget::{button, column, container, row, text}
};
use std::sync::{Arc, Mutex, mpsc};

use crate::{
    ui::threading,
    stream::rtsp
};

#[derive(Debug, Clone, Copy)]
pub enum Controls {
    InitializeWorker,
    WorkerInitialized,
    Next,
    Previous
}

#[derive(Default)]
pub struct Player {
    client: rtsp::Client,
    worker_sender: Arc<Mutex<Option<mpsc::Sender<Controls>>>>
}

impl Player {
    pub fn new(
        client: rtsp::Client,
        worker_sender: Arc<Mutex<Option<mpsc::Sender<Controls>>>>
    ) -> Self {
        Self {
            client: client,
            worker_sender: worker_sender
        }
    }

    pub fn boot() -> (Self, Task<Controls>) {
        (
            Player::new(rtsp::Client::new(vec![rtsp::Source{
                url: "".to_string(),
                username: None,
                password: None
            }]), Arc::new(Mutex::new(None))), // TODO: replace placeholder data with user input
            Task::done(Controls::InitializeWorker)
        )
    }

    pub fn update(&mut self, message: Controls) {
        log::debug!("{message:?}");

        match message {
            Controls::WorkerInitialized => { /* Nothing to do */ },
            _ => {
                // Relay all other messages to worker
                if let Some(sender) = self.worker_sender.lock().expect("Failed to lock sender").as_ref() {
                    let _ = sender.send(message);
                }
            }
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

    pub fn subscription(&self) -> Subscription<Controls> {
        subscription::from_recipe(threading::WorkerSubscription{ sender_holder: self.worker_sender.clone() })
    }
}
