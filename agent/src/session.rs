use std::path::PathBuf;

pub enum Entry {}

pub struct Session {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Session {
    pub fn new(path: PathBuf) -> Self {
        let entries = Vec::new();
        Self { path, entries }
    }

    pub fn append(&mut self, entry: Entry) {
        self.entries.push(entry);
    }
}
