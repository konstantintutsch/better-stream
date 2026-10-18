use iced::{
    Center, Element, Fill, Subscription, Task,
    advanced::subscription,
    widget::{button, column, container, image, row, text},
};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};

use crate::{config::Config, stream::rtsp, ui::threading};

const CONNECTING: &str = "Connecting …";
const ENDED: &str = "Stream ended";
const NO_SOURCES: &str = "No sources configured in";

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
    Reconnect,
    StreamFrame(image::Handle),
    StreamFailed(String),
    StreamEnded,
}

#[derive(Default)]
pub struct Player {
    client: Arc<Mutex<rtsp::Client>>,
    worker_sender: Arc<Mutex<Option<mpsc::Sender<Message>>>>,
    /// Most recently decoded picture, if the stream has delivered one
    frame: Option<image::Handle>,
    /// Shown while there is no picture to display
    status: String,
}

impl Player {
    pub fn new(
        client: Arc<Mutex<rtsp::Client>>,
        worker_sender: Arc<Mutex<Option<mpsc::Sender<Message>>>>,
    ) -> Self {
        let mut player = Self {
            client: client,
            worker_sender: worker_sender,
            frame: None,
            status: String::new(),
        };
        player.status = player.waiting();

        player
    }

    /// Retrieve readable and writable client from Arc<Mutex<>>
    pub fn client(&self) -> MutexGuard<'_, rtsp::Client> {
        self.client.as_ref().lock().expect("Failed to lock client")
    }

    pub fn boot() -> (Self, Task<Message>) {
        (
            Player::new(
                Arc::new(Mutex::new(rtsp::Client::new(Config::load().sources))),
                Arc::new(Mutex::new(None)),
            ),
            Task::done(Message::InitializeApp),
        )
    }

    pub fn update(&mut self, message: Message) {
        match &message {
            // Logging every picture would bury every other message
            Message::StreamFrame(_) => log::trace!("StreamFrame"),
            _ => log::debug!("{message:?}"),
        }

        match message {
            Message::InitializeApp => {
                self.send_worker(Message::InitializeWorker(self.client.clone()))
            }
            Message::WorkerInitialized => { /* Nothing to do */ }
            // The worker reconnects whenever it finishes, so wait for a new picture
            Message::WorkerFinished => self.reset(self.waiting()),
            Message::WorkerIgnored => { /* Nothing to do */ }
            Message::StreamFrame(frame) => self.frame = Some(frame),
            Message::StreamFailed(error) => self.reset(error),
            Message::StreamEnded => self.reset(ENDED.to_string()),
            _ => self.send_worker(message), // Relay all other messages to worker
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let source = self.client().current().map(|source| source.url.clone());

        let picture: Element<'_, Message> = match &self.frame {
            Some(frame) => image(frame).width(Fill).height(Fill).into(),
            None => container(text(self.status.as_str()))
                .center(Fill)
                .padding(10)
                .into(),
        };

        container(
            column![
                picture,
                text(source.unwrap_or_default()),
                row![
                    button("Previous").on_press(Message::Previous),
                    button("Reconnect").on_press(Message::Reconnect),
                    button("Next").on_press(Message::Next)
                ]
                .spacing(10)
            ]
            .align_x(Center)
            .spacing(10),
        )
        .padding(10)
        .center(Fill)
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        subscription::from_recipe(threading::WorkerSubscription {
            sender_holder: self.worker_sender.clone(),
        })
    }

    /// What to show while the selected source has not delivered a picture yet
    fn waiting(&self) -> String {
        match self.client().current() {
            Some(_) => CONNECTING.to_string(),
            None => format!("{NO_SOURCES} {}", Config::path().display()),
        }
    }

    /// Drop the displayed picture in favour of a status message
    fn reset(&mut self, status: String) {
        self.frame = None;
        self.status = status;
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
