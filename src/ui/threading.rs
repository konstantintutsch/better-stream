use iced::{
    advanced::subscription,
    futures::{SinkExt, Stream},
    stream,
};
use std::{
    future, hash,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, mpsc},
    thread,
};

use crate::{stream::rtsp, ui::app::Message};

const CHANNEL_SIZE: usize = 5;

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
            log::debug!("{message:?}");

            match message {
                Message::InitializeWorker(client) => {
                    self.client = Some(client);
                    self.send(Message::WorkerInitialized);
                    continue;
                }
                Message::Next => self.client().next(),
                Message::Previous => self.client().previous(),
                _ => {
                    self.send(Message::WorkerIgnored);
                    continue;
                }
            }

            self.send(Message::WorkerFinished);
        }
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
