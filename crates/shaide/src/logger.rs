use std::{
    fmt::{self, Write as FmtWrite},
    fs::{self, File},
    io::Write,
    sync::Mutex,
};

use chrono::Local;
use shaide_common::path::shaide_root;
use tracing::{
    Subscriber,
    field::{self, Visit},
};
use tracing_subscriber::{
    EnvFilter, Layer, Registry, layer::SubscriberExt, registry::LookupSpan, util::SubscriberInitExt,
};

struct FileLogger {
    file: Mutex<File>,
}

impl FileLogger {
    fn new() -> Self {
        let time_stamp = chrono::offset::Utc::now();
        let time_stamp = format!("{time_stamp:?}").replace("T", "_");
        let time_stamp = time_stamp.replace(":", "-");
        let log_file_name = format!("{}.log", time_stamp);
        let shaide_root = shaide_root();
        fs::create_dir_all(&shaide_root).unwrap();
        let file_name = shaide_root.join(log_file_name);
        let file = File::create(file_name).expect("Could not create log file");
        Self {
            file: Mutex::new(file),
        }
    }
}

#[derive(Debug)]
struct LogVisitor {
    method: Option<String>,
    uri: Option<String>,
    body: Option<String>,
    message: Option<String>,
}

impl LogVisitor {
    fn new() -> Self {
        Self {
            method: None,
            uri: None,
            body: None,
            message: None,
        }
    }

    fn to_log_line(&self) -> String {
        let mut log_line = String::from("\n");
        if let Some(method) = &self.method {
            writeln!(log_line, "\tmethod: {}", method).unwrap();
        }
        if let Some(uri) = &self.uri {
            writeln!(log_line, "\turi: {}", uri).unwrap();
        }
        if let Some(body) = &self.body
            && !body.is_empty()
        {
            writeln!(log_line, "\tbody: {}", body).unwrap();
        }
        if let Some(message) = &self.message {
            writeln!(log_line, "\tmessage: {}", message).unwrap();
        }
        log_line
    }
}

impl Visit for LogVisitor {
    fn record_str(&mut self, field: &field::Field, value: &str) {
        match field.name() {
            "uri" => {
                self.uri = Some(value.to_owned());
            }
            "method" => {
                self.method = Some(value.to_owned());
            }
            "body" => {
                self.body = Some(value.to_owned());
            }
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"))
        }
    }
}

impl<S> Layer<S> for FileLogger
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let date = Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
        let meta = event.metadata();
        let level = meta.level();
        let file = meta.file().unwrap_or("<unknown>");
        let line = meta.line().unwrap_or(0);

        let mut visitor = LogVisitor::new();
        event.record(&mut visitor);
        let log_line = format!("{date} {level} {file}:{line} {}", visitor.to_log_line());

        let mut file = self.file.lock().expect("Could not unlock mutex");
        file.write_all(log_line.as_bytes())
            .expect("Error writing to log file");
    }
}

pub fn init_tracing() {
    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = Vec::new();
    let file_logger = FileLogger::new();
    layers.push(Box::new(file_logger));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .boxed();
    layers.push(fmt_layer);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(layers)
        .with(env_filter)
        .init();
}
