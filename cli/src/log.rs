use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Stdout,
    Stderr,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    /// Empty string for step-level/global messages not tied to one node.
    pub node: String,
    pub level: LogLevel,
    pub text: String,
}

pub type LogSender = UnboundedSender<LogLine>;

pub fn log(tx: &LogSender, node: &str, level: LogLevel, text: impl Into<String>) {
    let _ = tx.send(LogLine {
        node: node.to_string(),
        level,
        text: text.into(),
    });
}
