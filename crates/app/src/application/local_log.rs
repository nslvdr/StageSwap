use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LOG_FILE_PREFIX: &str = "stageswap-";

pub struct LocalLog {
    directory: PathBuf,
    retention: Duration,
}

#[derive(Serialize)]
struct Entry<'a> {
    timestamp_unix_ms: u128,
    level: &'a str,
    component: &'a str,
    event: &'a str,
    message: &'a str,
}

impl LocalLog {
    pub fn new(directory: PathBuf, retention_days: u64) -> Self {
        let result = Self {
            directory,
            retention: Duration::from_secs(retention_days * 24 * 60 * 60),
        };
        result.prune();
        result
    }

    pub fn write(&self, level: &str, component: &str, event: &str, message: &str) {
        if fs::create_dir_all(&self.directory).is_err() {
            return;
        }
        let now = SystemTime::now();
        let millis = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let day = millis / 86_400_000;
        let path = self.directory.join(format!("{LOG_FILE_PREFIX}{day}.jsonl"));
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let entry = Entry {
            timestamp_unix_ms: millis,
            level,
            component,
            event,
            message,
        };
        if let Ok(mut bytes) = serde_json::to_vec(&entry) {
            bytes.push(b'\n');
            let _ = file.write_all(&bytes);
        }
    }

    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }

    pub fn clear(&self) -> std::io::Result<()> {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return Ok(());
        };
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    #[cfg(any(windows, test))]
    pub fn export_to(&self, destination: &std::path::Path) -> std::io::Result<()> {
        let mut sources = match fs::read_dir(&self.directory) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        sources.sort_by_key(|entry| entry.file_name());
        let mut output = fs::File::create(destination)?;
        for source in sources {
            std::io::copy(&mut fs::File::open(source.path())?, &mut output)?;
        }
        output.sync_all()
    }

    fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let expired = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > self.retention);
            if expired {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_exports_and_clears_local_jsonl_logs() {
        let directory = tempfile::tempdir().unwrap();
        let log = LocalLog::new(directory.path().join("logs"), 14);
        log.write("info", "test", "ONE", "first");
        log.write("warning", "test", "TWO", "second");
        let log_name = fs::read_dir(log.directory())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .into_owned();
        assert!(log_name.starts_with(LOG_FILE_PREFIX));
        let export = directory.path().join("export.jsonl");
        log.export_to(&export).unwrap();
        let contents = fs::read_to_string(export).unwrap();
        assert!(contents.contains("\"event\":\"ONE\""));
        assert!(contents.contains("\"event\":\"TWO\""));
        log.clear().unwrap();
        assert_eq!(fs::read_dir(log.directory()).unwrap().count(), 0);
    }
}
