use std::usize;
use iced::futures::SinkExt;

use crate::ui::app::Controls;

#[derive(Default, Clone)]
pub struct Source {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>
}

#[derive(Default, Clone)]
pub struct Client {
    pub sources: Vec<Source>,
    pub source: usize
}

impl Client {
    pub fn new(sources: Vec<Source>) -> Self {
        Client {
            sources: sources,
            source: usize::default()
        }
    }

    pub fn next(&mut self) {
        let last: usize = self.sources.len() - 1;
        let first: usize = 0;

        self.source = if self.source == last { first } else { self.source + 1 };
    }

    pub fn previous(&mut self) {
        let last: usize = self.sources.len() - 1;
        let first: usize = 0;

        self.source = if self.source == first { last } else { self.source - 1 };
    }

    pub fn current(&self) -> &Source {
        return self.sources.get(self.source).expect("Failed to get current source");
    }
}

pub struct Worker {
    initialized: bool,
    receiver: std::sync::mpsc::Receiver<Controls>,
    output: iced::futures::channel::mpsc::Sender<Controls>
}

impl Worker {
    pub fn new(
        receiver: std::sync::mpsc::Receiver<Controls>,
        output: iced::futures::channel::mpsc::Sender<Controls>
    ) -> Self {
        Self {
            initialized: false,
            receiver: receiver,
            output: output
        }
    }

    pub fn run(mut self) {
        while let Ok(message) = self.receiver.recv() {
            log::debug!("{message:?}");

            if !self.initialized && let Controls::InitializeWorker = message {
                self.initialized = true;
                let _ = iced::futures::executor::block_on(self.output.send(Controls::WorkerInitialized));
            }
        }
    }
}
