use crate::entry::Entry;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use tracing::warn;

pub struct Session {
    entries: Vec<Entry>,
    writer: BufWriter<File>,
}

impl Session {
    /// Open (or create) a session file at `path`.
    ///
    /// If the file already exists, its JSONL entries are loaded into memory.
    /// A partial trailing line (e.g. from a crash) is skipped with a warning.
    /// The file is then opened in append mode for subsequent writes.
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        // Load existing entries if the file exists.
        let mut entries = Vec::new();
        if path.exists() {
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
            let total = lines.len();

            for (i, line) in lines.iter().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Entry>(line) {
                    Ok(entry) => entries.push(entry),
                    Err(err) => {
                        if i == total - 1 {
                            // Tolerate a broken last line (crash mid-write).
                            warn!(
                                "skipping partial last line in {}: {err}",
                                path.display()
                            );
                        } else {
                            return Err(anyhow::anyhow!(
                                "{}:{}: failed to parse entry: {err}",
                                path.display(),
                                i + 1,
                            ));
                        }
                    }
                }
            }
        }

        // Open for appending (creates if missing).
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let writer = BufWriter::new(file);

        Ok(Self { entries, writer })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Append an entry to the in-memory list and persist it as a JSONL line.
    pub fn append(&mut self, entry: Entry) {
        // Best-effort write; if serialization or I/O fails we log and continue.
        match serde_json::to_string(&entry) {
            Ok(json) => {
                if let Err(err) = writeln!(self.writer, "{json}") {
                    warn!("failed to write session entry: {err}");
                } else if let Err(err) = self.writer.flush() {
                    warn!("failed to flush session file: {err}");
                }
            }
            Err(err) => {
                warn!("failed to serialize session entry: {err}");
            }
        }
        self.entries.push(entry);
    }
}
