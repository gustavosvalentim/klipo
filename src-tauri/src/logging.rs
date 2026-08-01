use log::kv::VisitSource;
use log::{LevelFilter, Log, Metadata, Record, SetLoggerError};
use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE_NAME: &str = "klipo.log";
const MAX_LOG_FILE_SIZE: u64 = 5 * 1024 * 1024;
const MAX_ROTATED_LOG_FILES: usize = 5;

struct FileLogger {
    state: Mutex<LoggerState>,
}

struct LoggerState {
    path: PathBuf,
    file: File,
    size: u64,
}

impl FileLogger {
    fn new(directory: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let path = directory.join(LOG_FILE_NAME);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();

        Ok(Self {
            state: Mutex::new(LoggerState { path, file, size }),
        })
    }

    fn rotate(state: &mut LoggerState) -> io::Result<()> {
        state.file.flush()?;

        let oldest = rotated_path(&state.path, MAX_ROTATED_LOG_FILES);
        remove_file_if_exists(&oldest)?;

        for index in (1..MAX_ROTATED_LOG_FILES).rev() {
            let source = rotated_path(&state.path, index);
            let destination = rotated_path(&state.path, index + 1);
            rename_if_exists(&source, &destination)?;
        }

        rename_if_exists(&state.path, &rotated_path(&state.path, 1))?;
        state.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&state.path)?;
        state.size = 0;

        Ok(())
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= LevelFilter::Debug
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = match format_record(record) {
            Ok(line) => line,
            Err(_) => return,
        };
        let line_size = line.len() as u64;

        let Ok(mut state) = self.state.lock() else {
            return;
        };

        if state.size > 0
            && state.size.saturating_add(line_size) > MAX_LOG_FILE_SIZE
            && Self::rotate(&mut state).is_err()
        {
            return;
        }

        if state.file.write_all(line.as_bytes()).is_ok() {
            state.size = state.size.saturating_add(line_size);
            let _ = state.file.flush();
        }
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            let _ = state.file.flush();
        }
    }
}

pub fn init(directory: &Path) -> io::Result<()> {
    let logger = FileLogger::new(directory)?;
    log::set_boxed_logger(Box::new(logger)).map_err(set_logger_error)?;
    log::set_max_level(LevelFilter::Debug);
    Ok(())
}

fn set_logger_error(error: SetLoggerError) -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, error.to_string())
}

fn format_record(record: &Record<'_>) -> Result<String, serde_json::Error> {
    let mut fields = Map::new();
    let mut visitor = FieldsVisitor {
        fields: &mut fields,
    };
    let _ = record.key_values().visit(&mut visitor);

    let mut entry = Map::new();
    entry.insert(
        "timestamp_ms".into(),
        Value::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        ),
    );
    entry.insert("level".into(), Value::String(record.level().to_string()));
    entry.insert("target".into(), Value::String(record.target().to_owned()));
    entry.insert("message".into(), Value::String(record.args().to_string()));
    entry.insert("fields".into(), Value::Object(fields));

    let mut line = serde_json::to_string(&Value::Object(entry))?;
    line.push('\n');
    Ok(line)
}

struct FieldsVisitor<'a> {
    fields: &'a mut Map<String, Value>,
}

impl<'kvs> VisitSource<'kvs> for FieldsVisitor<'_> {
    fn visit_pair(
        &mut self,
        key: log::kv::Key<'kvs>,
        value: log::kv::Value<'kvs>,
    ) -> Result<(), log::kv::Error> {
        let value =
            serde_json::to_value(&value).unwrap_or_else(|_| Value::String(format!("{value:?}")));
        self.fields.insert(key.as_str().to_owned(), value);
        Ok(())
    }
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    path.with_file_name(format!("klipo.{index}.log"))
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn rename_if_exists(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_record, FileLogger, LOG_FILE_NAME};
    use log::{Level, Log, Record};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_records_to_the_log_file() {
        let directory = std::env::temp_dir().join(format!(
            "klipo-logging-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let logger = FileLogger::new(&directory).unwrap();
        let args = format_args!("event");
        let record = Record::builder()
            .args(args)
            .level(Level::Info)
            .target("test")
            .build();

        logger.log(&record);
        logger.flush();

        let contents = fs::read_to_string(directory.join(LOG_FILE_NAME)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(value["message"], "event");
        drop(logger);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn formats_structured_fields_as_json_properties() {
        let fields = [("attempts", 2)];
        let args = format_args!("event");
        let record = Record::builder()
            .args(args)
            .level(Level::Info)
            .target("test")
            .key_values(&fields)
            .build();

        let line = format_record(&record).unwrap();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();

        assert_eq!(value["message"], "event");
        assert_eq!(value["fields"]["attempts"], 2);
    }
}
