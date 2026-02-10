use iced::{
    Element, Fill, Subscription, Task,
    advanced::subscription,
    widget::{button, column, container, row, text},
};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};

use crate::{stream::rtsp, ui::threading};

/// Provides an internal communications framework within the interface and between the interface and its' workers.
#[derive(Debug, Clone)]
pub enum Message {
    InitializeApp,
    InitializeWorker(Arc<Mutex<rtsp::Client>>),
    WorkerInitialized,
    WorkerFinished,
    WorkerIgnored,
    Next,
    Previous,
}

#[derive(Default)]
pub struct Player {
    client: Arc<Mutex<rtsp::Client>>,
    worker_sender: Arc<Mutex<Option<mpsc::Sender<Message>>>>,
}

impl Player {
    pub fn new(
        client: Arc<Mutex<rtsp::Client>>,
        worker_sender: Arc<Mutex<Option<mpsc::Sender<Message>>>>,
    ) -> Self {
        Self {
            client: client,
            worker_sender: worker_sender,
        }
    }

    /// Retrieve readable and writable client from Arc<Mutex<>>
    pub fn client(&self) -> MutexGuard<'_, rtsp::Client> {
        self.client.as_ref().lock().expect("Failed to lock client")
    }

    pub fn boot() -> (Self, Task<Message>) {
        (
            Player::new(
                Arc::new(Mutex::new(rtsp::Client::new(vec![
                    rtsp::Source {
                        url: "first".to_string(),
                        username: None,
                        password: None,
                    },
                    rtsp::Source {
                        url: "middle".to_string(),
                        username: None,
                        password: None,
                    },
                    rtsp::Source {
                        url: "last".to_string(),
                        username: None,
                        password: None,
                    },
                ]))),
                Arc::new(Mutex::new(None)),
            ), // TODO: replace placeholder data with user input
            Task::done(Message::InitializeApp),
        )
    }

    pub fn update(&mut self, message: Message) {
        log::debug!("{message:?}");

        match message {
            Message::InitializeApp => {
                self.send_worker(Message::InitializeWorker(self.client.clone()))
            }
            Message::WorkerInitialized => { /* Nothing to do */ }
            Message::WorkerFinished => { /* Nothing to do */ }
            Message::WorkerIgnored => { /* Nothing to do */ }
            _ => self.send_worker(message), // Relay all other messages to worker
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        container(
            column![
                text(self.client().current().url.clone()),
                row![
                    button("Previous").on_press(Message::Previous),
                    button("Next").on_press(Message::Next)
                ]
                .spacing(10)
            ]
            .spacing(10),
        )
        .center(Fill)
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        subscription::from_recipe(threading::WorkerSubscription {
            sender_holder: self.worker_sender.clone(),
        })
    }

    /// Send message to stream worker
    pub fn send_worker(&mut self, message: Message) {
        if let Some(sender) = self
            .worker_sender
            .lock()
            .expect("Failed to lock sender")
            .as_ref()
        {
            let _ = sender.send(message);
        }
    }
}
