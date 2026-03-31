use crate::entry::Entry;
use std::path::PathBuf;

pub struct Session {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Session {
    pub fn new(path: PathBuf) -> Self {
        let entries = Vec::new();
        Self { path, entries }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn append(&mut self, entry: Entry) {
        self.entries.push(entry);
    }
}
