use iced::{
    Center, Element, Fill, Subscription, Task,
    advanced::subscription,
    widget::{button, column, container, image, row, text},
};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};

use crate::{config::Config, stream::rtsp, ui::threading};

const CONNECTING: &str = "Verbindung wird aufgebaut...";
const ENDED: &str = "Stream beendet";
const NO_SOURCES: &str = "Keine Quellen konfiguriert in";

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
    /// The renderer finished uploading a picture, tagged with the generation it was requested in
    FrameAllocated(u64, Result<image::Allocation, image::Error>),
    StreamFailed(String),
    StreamEnded,
}

#[derive(Default)]
pub struct Player {
    client: Arc<Mutex<rtsp::Client>>,
    worker_sender: Arc<Mutex<Option<mpsc::Sender<Message>>>>,
    /// Most recently decoded picture that is ready to be drawn, if the stream has delivered one
    frame: Option<image::Allocation>,
    /// A picture is being uploaded, newer ones are dropped until it is done
    allocating: bool,
    /// Incremented by every reset, so uploads started before it are discarded
    generation: u64,
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
            allocating: false,
            generation: 0,
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match &message {
            // Logging every picture would bury every other message
            Message::StreamFrame(_) => log::trace!("StreamFrame"),
            Message::FrameAllocated(generation, result) => {
                log::trace!("FrameAllocated({generation}, ok: {})", result.is_ok())
            }
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
            // Large pictures are uploaded in the background and not drawn until
            // that is done, so a new handle per picture would flicker. Keep the
            // current picture on screen until its successor is ready.
            Message::StreamFrame(frame) => {
                if self.allocating {
                    log::trace!("Dropped picture, previous one still uploading");
                } else {
                    self.allocating = true;
                    let generation = self.generation;

                    return image::allocate(frame)
                        .map(move |result| Message::FrameAllocated(generation, result));
                }
            }
            Message::FrameAllocated(generation, result) => {
                if generation == self.generation {
                    self.allocating = false;

                    match result {
                        Ok(frame) => self.frame = Some(frame),
                        Err(error) => log::warn!("Failed to upload picture: {error:?}"),
                    }
                }
            }
            Message::StreamFailed(error) => self.reset(error),
            Message::StreamEnded => self.reset(ENDED.to_string()),
            _ => self.send_worker(message), // Relay all other messages to worker
        }

        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let source = self.client().current().map(|source| {
    source.name.clone().unwrap_or_else(|| source.url.clone())
});

        let picture: Element<'_, Message> = match &self.frame {
            Some(frame) => image(frame.handle()).width(Fill).height(Fill).into(),
            None => container(text(self.status.as_str()))
                .center(Fill)
                .padding(10)
                .into(),
        };

        container(
            column![
                picture,
                row![
                    button("◀").on_press(Message::Previous),
                    text(source.unwrap_or_default()),
                    button("↻").on_press(Message::Reconnect),
                    button("▶").on_press(Message::Next)
                ]
                .align_y(Center)
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
        self.allocating = false;
        self.generation += 1;
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
