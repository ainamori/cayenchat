// SPDX-License-Identifier: MPL-2.0
//! Opt-in conversation logs: UTF-8, tab-separated, one file per conversation
//! and day, appended to by a thread of its own.
//!
//! ```text
//! <directory>/<network>/channels/#rust/2026-10-12.log
//! <directory>/<network>/private/alice/2026-10-12.log
//! ```
//!
//! Each file starts with `# cayenchat-log 1`; every other line has four
//! tab-separated columns: the time (ISO 8601 with milliseconds and offset),
//! the kind ([`Kind::label`]), the nickname and the text. Readers should skip
//! lines starting with `#` and lines whose kind they do not know.
//!
//! Nothing here knows IRC: the caller names the conversation (already folded
//! with the server's case mapping) and passes the text without formatting
//! codes. The log is for people and their tools, not for this application to
//! read back.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};

use chrono::{DateTime, Local, TimeZone};

use crate::private_file;

/// The first line of every log file.
const HEADER: &str = "# cayenchat-log 1\n";
/// Held by the CayenChat that writes to a directory.
const LOCK_FILE: &str = ".cayenchat-log.lock";
/// Records waiting for the writer; more are dropped (and counted).
const QUEUE_LIMIT: usize = 4096;
/// How often written lines are flushed to the disk.
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
/// Files kept open at once; all are closed when more are needed.
const OPEN_FILES: usize = 32;
/// Longest path component in bytes, below the 255 most file systems allow.
const COMPONENT_LIMIT: usize = 200;

/// What a line says. The label is the second column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Msg,
    Action,
    Notice,
    Join,
    Part,
    Quit,
    Nick,
    Kick,
    Topic,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Msg => "msg",
            Self::Action => "action",
            Self::Notice => "notice",
            Self::Join => "join",
            Self::Part => "part",
            Self::Quit => "quit",
            Self::Nick => "nick",
            Self::Kick => "kick",
            Self::Topic => "topic",
        }
    }
}

/// Which directory below the network a conversation is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Channel,
    Private,
}

impl Scope {
    fn directory(self) -> &'static str {
        match self {
            Self::Channel => "channels",
            Self::Private => "private",
        }
    }
}

/// One line to log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The server's time when it gave one, else the receipt time.
    pub time: SystemTime,
    /// The server's name as shown, made unique by the caller.
    pub network: String,
    pub scope: Scope,
    /// The channel or peer, folded with the server's case mapping.
    pub conversation: String,
    pub kind: Kind,
    pub nick: String,
    /// Without IRC formatting codes.
    pub body: String,
}

/// The default directory: `<data directory>/CayenChat/logs`.
pub fn default_directory() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("CayenChat").join("logs"))
}

/// Makes `name` one safe path component on every OS. Characters no OS
/// accepts (`/ \ : * ? " < > |`, controls) and `%` become `%XX`, as do a
/// name that is only dots, a trailing dot or space and the first letter of a
/// Windows device name (`CON`, `NUL`, `COM1`…). An empty name becomes `%00`.
/// The result is never a path of its own, so a name from a server cannot
/// reach outside the directory it is placed in.
pub fn path_component(name: &str) -> String {
    fn push_escaped(out: &mut String, ch: char) {
        let mut buffer = [0; 4];
        for byte in ch.encode_utf8(&mut buffer).bytes() {
            out.push_str(&format!("%{byte:02X}"));
        }
    }

    let chars: Vec<char> = name.chars().collect();
    let stem = name.split('.').next().unwrap_or("");
    let device = is_device_name(stem);
    let dots = !name.is_empty() && name.chars().all(|ch| ch == '.');
    let mut out = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        let unsafe_char = matches!(
            ch,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '%'
        ) || ch.is_control();
        let last = index + 1 == chars.len();
        let escape =
            unsafe_char || dots || (device && index == 0) || (last && (ch == '.' || ch == ' '));
        // Escaped names can grow threefold; stop before the limit rather
        // than in the middle of an escape.
        let mut piece = String::new();
        if escape {
            push_escaped(&mut piece, ch);
        } else {
            piece.push(ch);
        }
        if out.len() + piece.len() > COMPONENT_LIMIT {
            break;
        }
        out.push_str(&piece);
    }
    if out.is_empty() {
        out.push_str("%00");
    }
    // A cut name may now end in a dot or a space.
    if out.ends_with(['.', ' ']) {
        let ch = out.pop().unwrap_or('.');
        push_escaped(&mut out, ch);
    }
    out
}

fn is_device_name(stem: &str) -> bool {
    let upper = stem.trim_end().to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

/// The file `record` belongs to: the day is the record's date in `zone`.
pub fn path_for<Tz: TimeZone>(root: &Path, record: &Record, zone: &Tz) -> PathBuf
where
    Tz::Offset: std::fmt::Display,
{
    let date = DateTime::<chrono::Utc>::from(record.time).with_timezone(zone);
    root.join(path_component(&record.network))
        .join(record.scope.directory())
        .join(path_component(&record.conversation))
        .join(format!("{}.log", date.format("%Y-%m-%d")))
}

/// Replaces what would break the four columns. Tabs become spaces and other
/// control characters (a newline included) U+FFFD. With `spaces`, the
/// whitespace of a nickname is replaced as well.
fn cell(text: &str, spaces: bool) -> String {
    text.chars()
        .map(|ch| match ch {
            '\t' if !spaces => ' ',
            ch if ch.is_control() || (spaces && ch.is_whitespace()) => '\u{FFFD}',
            ch => ch,
        })
        .collect()
}

/// The line for `record` in `zone`, with its newline.
pub fn line_for<Tz: TimeZone>(record: &Record, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let time = DateTime::<chrono::Utc>::from(record.time).with_timezone(zone);
    format!(
        "{}\t{}\t{}\t{}\n",
        time.format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
        record.kind.label(),
        cell(&record.nick, true),
        cell(&record.body, false),
    )
}

#[derive(Default)]
struct Shared {
    /// Another CayenChat holds the directory; nothing is written.
    locked: AtomicBool,
    dropped: AtomicU64,
    failure: Mutex<Option<String>>,
    announced: AtomicBool,
}

/// How a logger is doing, for the settings and the server tabs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Another CayenChat is writing to the directory, so this one is not.
    pub locked: bool,
    /// The first error, kept until the logger is replaced.
    pub failure: Option<String>,
    /// Records dropped because the writer could not keep up.
    pub dropped: u64,
}

/// Writes records on a thread of its own. Dropping it writes what is queued,
/// flushes and waits for the thread.
pub struct ChatLogger {
    sender: Option<SyncSender<Record>>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl ChatLogger {
    /// Starts logging into `directory` (created when missing). Opening it
    /// happens on the writer thread, so this never waits for the disk. A
    /// directory that another CayenChat has locked is not written to (see
    /// [`Status::locked`]); an error is reported through [`Status::failure`].
    pub fn start(directory: PathBuf) -> Self {
        let shared = Arc::new(Shared::default());
        let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        let worker_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("chat-log".into())
            .spawn(move || match acquire(&directory) {
                Ok(Some(lock)) => Writer {
                    root: directory,
                    shared: worker_shared,
                    files: HashMap::new(),
                    _lock: lock,
                }
                .run(receiver),
                Ok(None) => worker_shared.locked.store(true, Ordering::Relaxed),
                Err(error) => fail(&worker_shared, &directory, &error),
            })
            .ok();
        Self {
            sender: thread.is_some().then_some(sender),
            shared,
            thread,
        }
    }

    /// Queues `record`; never waits. A full queue drops it and counts it.
    pub fn log(&self, record: Record) {
        let Some(sender) = &self.sender else {
            return;
        };
        // A writer that ended (locked directory, failed start) has already
        // said why; only a full queue is counted.
        if let Err(TrySendError::Full(_)) = sender.try_send(record) {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn status(&self) -> Status {
        Status {
            locked: self.shared.locked.load(Ordering::Relaxed),
            failure: self
                .shared
                .failure
                .lock()
                .ok()
                .and_then(|failure| failure.clone()),
            dropped: self.shared.dropped.load(Ordering::Relaxed),
        }
    }

    /// The first failure, once: `Some` the first time it is asked after the
    /// failure, so it is announced a single time.
    pub fn take_announcement(&self) -> Option<String> {
        let failure = self.status().failure?;
        (!self.shared.announced.swap(true, Ordering::Relaxed)).then_some(failure)
    }
}

impl Drop for ChatLogger {
    fn drop(&mut self) {
        self.sender = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// `Ok(None)` when another process holds the directory.
fn acquire(directory: &Path) -> io::Result<Option<File>> {
    private_file::create_dir(directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(LOCK_FILE))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

fn fail(shared: &Shared, path: &Path, error: &io::Error) {
    if let Ok(mut failure) = shared.failure.lock()
        && failure.is_none()
    {
        *failure = Some(format!("{}: {error}", path.display()));
    }
}

struct Writer {
    root: PathBuf,
    shared: Arc<Shared>,
    files: HashMap<PathBuf, BufWriter<File>>,
    /// The lock lives as long as the writer.
    _lock: File,
}

impl Writer {
    fn run(mut self, receiver: mpsc::Receiver<Record>) {
        let mut flush_at = Instant::now() + FLUSH_INTERVAL;
        loop {
            let wait = flush_at.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(wait) {
                Ok(record) => self.write(&record),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if Instant::now() >= flush_at {
                self.flush();
                flush_at = Instant::now() + FLUSH_INTERVAL;
            }
        }
        self.flush();
    }

    fn write(&mut self, record: &Record) {
        let path = path_for(&self.root, record, &Local);
        let line = line_for(record, &Local);
        if let Err(error) = self.append(&path, &line) {
            // Reopened at the next record.
            self.files.remove(&path);
            fail(&self.shared, &path, &error);
        }
    }

    fn append(&mut self, path: &Path, line: &str) -> io::Result<()> {
        if !self.files.contains_key(path) {
            if self.files.len() >= OPEN_FILES {
                self.flush();
                self.files.clear();
            }
            let file = open(path)?;
            self.files.insert(path.to_owned(), file);
        }
        match self.files.get_mut(path) {
            Some(file) => file.write_all(line.as_bytes()),
            None => Ok(()),
        }
    }

    fn flush(&mut self) {
        let mut broken = Vec::new();
        for (path, file) in &mut self.files {
            let result = file.flush().and_then(|()| file.get_ref().sync_data());
            if let Err(error) = result {
                fail(&self.shared, path, &error);
                broken.push(path.clone());
            }
        }
        for path in broken {
            self.files.remove(&path);
        }
    }
}

/// Opens `path` for appending (user-only, creating the directories) and
/// writes the header into a new file. A symbolic link in place of the file
/// is refused, not followed.
fn open(path: &Path) -> io::Result<BufWriter<File>> {
    if let Some(parent) = path.parent() {
        private_file::create_dir(parent)?;
    }
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the log file is a symbolic link",
        ));
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let mut file = BufWriter::new(file);
    if file.get_ref().metadata()?.len() == 0 {
        file.write_all(HEADER.as_bytes())?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    fn record(kind: Kind, nick: &str, body: &str) -> Record {
        Record {
            // 2026-10-12T08:02:13.120Z
            time: SystemTime::UNIX_EPOCH + Duration::from_millis(1_791_792_133_120),
            network: "Libera".into(),
            scope: Scope::Channel,
            conversation: "#rust".into(),
            kind,
            nick: nick.into(),
            body: body.into(),
        }
    }

    fn tokyo() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    #[test]
    fn a_line_has_four_columns_in_the_local_time() {
        let line = line_for(&record(Kind::Msg, "alice", "hello"), &tokyo());
        assert_eq!(line, "2026-10-12T17:02:13.120+09:00\tmsg\talice\thello\n");
    }

    #[test]
    fn text_cannot_add_columns_or_lines() {
        let line = line_for(&record(Kind::Msg, "al ice\t", "a\tb\r\nc\u{1b}d"), &tokyo());
        let columns: Vec<_> = line.trim_end_matches('\n').split('\t').collect();
        assert_eq!(columns.len(), 4);
        assert_eq!(columns[2], "al\u{FFFD}ice\u{FFFD}");
        assert_eq!(columns[3], "a b\u{FFFD}\u{FFFD}c\u{FFFD}d");
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn the_file_is_chosen_by_the_local_date() {
        let root = Path::new("logs");
        let mut late = record(Kind::Join, "bob", "");
        late.time += Duration::from_secs(8 * 3600);
        assert_eq!(
            path_for(root, &late, &tokyo()),
            Path::new("logs/Libera/channels/#rust/2026-10-13.log")
        );
        assert_eq!(
            path_for(root, &late, &FixedOffset::east_opt(0).unwrap()),
            Path::new("logs/Libera/channels/#rust/2026-10-12.log")
        );
        let mut private = record(Kind::Msg, "alice", "hi");
        private.scope = Scope::Private;
        private.conversation = "alice".into();
        assert!(path_for(root, &private, &tokyo()).starts_with("logs/Libera/private/alice"));
    }

    #[test]
    fn names_are_one_safe_component() {
        assert_eq!(path_component("#rust"), "#rust");
        assert_eq!(path_component("日本語"), "日本語");
        assert_eq!(
            path_component("a/b\\c:d*e?f\"g<h>i|j"),
            "a%2Fb%5Cc%3Ad%2Ae%3Ff%22g%3Ch%3Ei%7Cj"
        );
        assert_eq!(path_component("100%"), "100%25");
        assert_eq!(path_component("tab\there"), "tab%09here");
        assert_eq!(path_component(".."), "%2E%2E");
        assert_eq!(path_component("."), "%2E");
        assert_eq!(path_component("name."), "name%2E");
        assert_eq!(path_component("name "), "name%20");
        assert_eq!(path_component("CON"), "%43ON");
        assert_eq!(path_component("nul.txt"), "%6Eul.txt");
        assert_eq!(path_component("com1"), "%63om1");
        assert_eq!(path_component("console"), "console");
        assert_eq!(path_component(""), "%00");
        assert_eq!(path_component("../../etc"), "..%2F..%2Fetc");
    }

    #[test]
    fn long_names_are_cut_between_characters() {
        let long = path_component(&"é".repeat(300));
        assert!(long.len() <= COMPONENT_LIMIT);
        let escaped = path_component(&"/".repeat(300));
        assert!(escaped.len() <= COMPONENT_LIMIT);
        assert!(escaped.len().is_multiple_of(3) && escaped.starts_with("%2F"));
    }

    #[test]
    fn records_are_appended_to_one_file_with_a_single_header() {
        let directory = tempfile::tempdir().unwrap();
        let first = ChatLogger::start(directory.path().to_owned());
        first.log(record(Kind::Msg, "alice", "one"));
        drop(first);
        let second = ChatLogger::start(directory.path().to_owned());
        second.log(record(Kind::Action, "bob", "two"));
        let status = second.status();
        assert!(!status.locked && status.failure.is_none());
        drop(second);

        let path = path_for(directory.path(), &record(Kind::Msg, "", ""), &Local);
        let text = fs::read_to_string(path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines[0], "# cayenchat-log 1");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with("\tmsg\talice\tone"));
        assert!(lines[2].ends_with("\taction\tbob\ttwo"));
    }

    #[test]
    fn a_second_logger_on_a_directory_does_not_write() {
        let directory = tempfile::tempdir().unwrap();
        let first = ChatLogger::start(directory.path().to_owned());
        // The first one holds the lock once its thread has opened it.
        wait_for(|| directory.path().join(LOCK_FILE).exists());
        std::thread::sleep(Duration::from_millis(50));
        let second = ChatLogger::start(directory.path().to_owned());
        wait_for(|| second.status().locked);
        assert!(second.status().locked);
        second.log(record(Kind::Msg, "alice", "ignored"));
        drop(second);
        first.log(record(Kind::Msg, "alice", "kept"));
        drop(first);
        let path = path_for(directory.path(), &record(Kind::Msg, "", ""), &Local);
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("kept") && !text.contains("ignored"));
        // Released with the first logger.
        let third = ChatLogger::start(directory.path().to_owned());
        third.log(record(Kind::Msg, "alice", "again"));
        drop(third);
        let text = fs::read_to_string(path_for(
            directory.path(),
            &record(Kind::Msg, "", ""),
            &Local,
        ))
        .unwrap();
        assert!(text.contains("again"));
    }

    #[test]
    fn a_failure_is_kept_and_announced_once() {
        let directory = tempfile::tempdir().unwrap();
        // A file where the network's directory must be.
        fs::write(directory.path().join("Libera"), b"").unwrap();
        let logger = ChatLogger::start(directory.path().to_owned());
        logger.log(record(Kind::Msg, "alice", "lost"));
        drop_after_write(&logger);
        assert!(logger.status().failure.is_some());
        assert!(logger.take_announcement().is_some());
        assert!(logger.take_announcement().is_none());
    }

    /// Waits until `logger` reports a failure.
    fn drop_after_write(logger: &ChatLogger) {
        wait_for(|| logger.status().failure.is_some());
    }

    fn wait_for(condition: impl Fn() -> bool) {
        for _ in 0..500 {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_in_place_of_the_file_is_not_followed() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.txt");
        fs::write(&target, b"keep\n").unwrap();
        let path = path_for(
            &directory.path().join("logs"),
            &record(Kind::Msg, "", ""),
            &Local,
        );
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let logger = ChatLogger::start(directory.path().join("logs"));
        logger.log(record(Kind::Msg, "alice", "never"));
        drop_after_write(&logger);
        assert!(logger.status().failure.is_some());
        drop(logger);
        assert_eq!(fs::read(&target).unwrap(), b"keep\n");
    }

    #[cfg(unix)]
    #[test]
    fn files_and_directories_are_for_the_user_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("logs");
        let logger = ChatLogger::start(root.clone());
        logger.log(record(Kind::Msg, "alice", "private"));
        drop(logger);
        let path = path_for(&root, &record(Kind::Msg, "", ""), &Local);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&root), 0o700);
    }
}
