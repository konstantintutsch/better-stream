use iced::{
    advanced::subscription,
    futures::{SinkExt, Stream},
    stream,
    widget::image,
};
use std::{
    future, hash,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, mpsc},
    thread,
};
use tokio::{runtime::Runtime, task::JoinHandle};

use crate::{stream::rtsp, ui::app::Message};

const CHANNEL_SIZE: usize = 5;
/// Only the newest picture is worth showing, so barely buffer any
const FRAME_SIZE: usize = 1;

#[derive(Clone)]
pub struct WorkerSubscription {
    pub sender_holder: Arc<Mutex<Option<mpsc::Sender<Message>>>>,
}

impl hash::Hash for WorkerSubscription {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        std::any::TypeId::of::<WorkerSubscription>().hash(state);
    }
}

impl iced::advanced::subscription::Recipe for WorkerSubscription {
    type Output = Message;

    fn hash(&self, state: &mut subscription::Hasher) {
        hash::Hash::hash(self, state);
    }

    fn stream(
        self: Box<Self>,
        _input: Pin<Box<dyn Stream<Item = subscription::Event> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = Self::Output> + Send>> {
        Box::pin(stream::channel(CHANNEL_SIZE, move |output| async move {
            let (sender, receiver) = mpsc::channel();
            *self.sender_holder.lock().expect("Failed to lock sender") = Some(sender);

            thread::spawn(move || {
                Worker::new(receiver, output).run();
            });

            future::pending::<()>().await;
        }))
    }
}

pub struct Worker {
    receiver: mpsc::Receiver<Message>,
    output: iced::futures::channel::mpsc::Sender<Message>,
    client: Option<Arc<Mutex<rtsp::Client>>>,
    /// Owned by the worker, because iced does not run on a tokio runtime
    runtime: Runtime,
    stream: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn new(
        receiver: mpsc::Receiver<Message>,
        output: iced::futures::channel::mpsc::Sender<Message>,
    ) -> Self {
        Self {
            receiver: receiver,
            output: output,
            client: None,
            runtime: Runtime::new().expect("Failed to create runtime"),
            stream: None,
        }
    }

    /// Retrieve readable and writable client from Option<Arc<Mutex<>>>
    pub fn client(&self) -> MutexGuard<'_, rtsp::Client> {
        self.client
            .as_ref()
            .expect("Failed to get client")
            .lock()
            .expect("Failed to lock client")
    }

    pub fn run(&mut self) {
        while let Ok(message) = self.receiver.recv() {
            match message {
                Message::InitializeWorker(client) => {
                    self.client = Some(client);
                    self.play();
                    self.send(Message::WorkerInitialized);
                    continue;
                }
                Message::Next => {
                    self.client().next();
                    self.play();
                }
                Message::Previous => {
                    self.client().previous();
                    self.play();
                }
                Message::Reconnect => self.play(),
                _ => {
                    self.send(Message::WorkerIgnored);
                    continue;
                }
            }

            self.send(Message::WorkerFinished);
        }
    }

    /// Drop the running stream and connect to the currently selected source
    fn play(&mut self) {
        if let Some(stream) = self.stream.take() {
            stream.abort();
        }

        let Some(source) = self.client().current().cloned() else {
            return;
        };

        let mut output = self.output.clone();
        let mut frames = self.output.clone();

        self.stream = Some(self.runtime.spawn(async move {
            let (sender, mut receiver) = tokio::sync::mpsc::channel::<rtsp::Frame>(FRAME_SIZE);

            // Runs alongside the connection and ends once it drops the sender
            let forward = async move {
                while let Some(frame) = receiver.recv().await {
                    let handle = image::Handle::from_rgba(frame.width, frame.height, frame.pixels);

                    if frames.send(Message::StreamFrame(handle)).await.is_err() {
                        break;
                    }
                }
            };

            let (result, ()) = iced::futures::join!(rtsp::play(source, sender), forward);

            let message = match result {
                Ok(()) => Message::StreamEnded,
                Err(error) => Message::StreamFailed(error.to_string()),
            };

            let _ = output.send(message).await;
        }));
    }

    /// Send message to output
    pub fn send(&mut self, message: Message) {
        let result = iced::futures::executor::block_on(self.output.send(message));

        match result {
            Ok(()) => {}
            Err(error) => log::error!("Failed to send message to output: {error:?}"),
        }
    }
}
