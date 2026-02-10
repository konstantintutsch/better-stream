use iced::{advanced::subscription, futures::Stream, stream};
use std::{
    future, hash, pin::Pin, sync::{Arc, Mutex, mpsc}, thread
};

use crate::{
    ui::app::Controls,
    stream::rtsp
};

const CHANNEL_SIZE: usize = 5;

#[derive(Clone)]
pub struct WorkerSubscription {
    pub sender_holder: Arc<Mutex<Option<mpsc::Sender<Controls>>>>,
}

impl hash::Hash for WorkerSubscription {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        std::any::TypeId::of::<WorkerSubscription>().hash(state);
    }
}

impl iced::advanced::subscription::Recipe for WorkerSubscription {
    type Output = Controls;

    fn hash(&self, state: &mut subscription::Hasher) {
        hash::Hash::hash(self, state);
    }

    fn stream(
        self: Box<Self>,
        _input: Pin<Box<dyn Stream<Item = subscription::Event> + Send>>
    ) -> Pin<Box<dyn Stream<Item = Self::Output> + Send>> {
        Box::pin(stream::channel(CHANNEL_SIZE, move |output| {
            async move {
                let (sender, receiver) = mpsc::channel();
                *self.sender_holder.lock().expect("Failed to lock sender") = Some(sender);

                thread::spawn(move || {
                    rtsp::Worker::new(receiver, output)
                        .run();
                });

                future::pending::<()>().await;
            }
        }))
    }
}
