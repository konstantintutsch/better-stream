use std::usize;

#[derive(Default, Clone)]
pub struct Source {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>
}

#[derive(Default)]
pub struct Client {
    pub sources: Vec<Source>,
    pub source: usize
}

impl Client {
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
