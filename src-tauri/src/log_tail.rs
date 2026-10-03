use std::path::PathBuf;

pub struct TailChunk {
    pub text: String,
    /// The file this text came from is not the one the previous chunk came
    /// from. Readers holding state across chunks must drop it.
    pub restarted: bool,
}

#[cfg(windows)]
type FileId = (u32, u32, u32);

#[cfg(windows)]
fn file_id(file: &std::fs::File) -> std::io::Result<FileId> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // The handle stays open for this call; the API initializes info on success.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    Ok((
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    ))
}

#[cfg(unix)]
type FileId = (u64, u64, Option<std::time::SystemTime>);

#[cfg(unix)]
fn file_id(file: &std::fs::File) -> std::io::Result<FileId> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    Ok((meta.dev(), meta.ino(), meta.created().ok()))
}

pub struct LogTail {
    path: PathBuf,
    pos: u64,
    /// Identity of the file `pos` counts into. A new launch writes a new file
    /// at the same path, and its first bytes are a boot header that readers
    /// must not miss.
    file_id: Option<FileId>,
    /// The leading bytes of a character whose remaining bytes have not been
    /// written yet.
    partial_char: Vec<u8>,
    /// Set when the file changed underneath us, cleared once a chunk has
    /// carried the fact to the caller. It outlives the read that noticed it
    /// because that read can come back empty.
    restarted: bool,
}

impl LogTail {
    /// Empty logs still establish the boundary between backfill and live events.
    pub fn has_read(&self) -> bool {
        self.file_id.is_some()
    }

    pub fn from_start(path: PathBuf) -> Self {
        Self {
            path,
            pos: 0,
            file_id: None,
            partial_char: Vec::new(),
            restarted: false,
        }
    }

    /// Starts at the end, for readers that react to events as they happen and
    /// would fire on hours-old lines if handed the existing log.
    pub fn from_end(path: PathBuf) -> Self {
        let seen = std::fs::File::open(&path)
            .ok()
            .and_then(|file| Some((file.metadata().ok()?.len(), file_id(&file).ok()?)));
        Self {
            pos: seen.as_ref().map_or(0, |(len, _)| *len),
            file_id: seen.map(|(_, id)| id),
            path,
            partial_char: Vec::new(),
            restarted: false,
        }
    }

    /// Whatever has been appended since the last call, or `None` when the file
    /// is unreadable or has not grown.
    pub fn read(&mut self) -> Option<TailChunk> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = std::fs::File::open(&self.path).ok()?;
        let meta = file.metadata().ok()?;
        // Identity and bytes must come from the same open file. A failed identity
        // query leaves the cursor untouched so the next filesystem event can retry.
        let id = file_id(&file).ok()?;
        let restarted = self.file_id.is_some_and(|seen| seen != id) || meta.len() < self.pos;
        let pos = if restarted { 0 } else { self.pos };
        file.seek(SeekFrom::Start(pos)).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;

        if restarted {
            self.partial_char.clear();
            self.restarted = true;
        }
        self.file_id = Some(id);
        // Reading may include bytes appended after the metadata snapshot.
        self.pos = pos + bytes.len() as u64;

        let text = self.decode(&bytes);
        if text.is_empty() {
            return None;
        }
        Some(TailChunk {
            text,
            restarted: std::mem::take(&mut self.restarted),
        })
    }

    /// A read can land mid-character, and a log can hold a byte that is no
    /// character at all. The first waits for the rest of its character; the
    /// second is dropped, because a cursor that refuses to pass one bad byte
    /// stops delivering the file for as long as the byte is in it.
    fn decode(&mut self, bytes: &[u8]) -> String {
        let mut buf = std::mem::take(&mut self.partial_char);
        buf.extend_from_slice(bytes);

        let mut text = String::new();
        let mut at = 0;
        loop {
            match std::str::from_utf8(&buf[at..]) {
                Ok(rest) => {
                    text.push_str(rest);
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    text.push_str(
                        std::str::from_utf8(&buf[at..at + valid])
                            .expect("valid_up_to reports a decodable prefix"),
                    );
                    match e.error_len() {
                        Some(bad) => at += valid + bad,
                        None => {
                            self.partial_char = buf[at + valid..].to_vec();
                            break;
                        }
                    }
                }
            }
        }
        text
    }
}

#[cfg(test)]
mod log_tail_tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn empty_initial_read_establishes_backfill_boundary() {
        let path = scratch("empty-initial.log");
        let mut tail = LogTail::from_start(path.clone());
        assert!(tail.read().is_none());
        assert!(!tail.has_read());
        append(&path, b"");
        assert!(tail.read().is_none());
        assert!(tail.has_read());
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new("tmp").join(format!("frameforge-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir is always writable");
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    fn append(path: &Path, bytes: &[u8]) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("scratch file is writable");
        f.write_all(bytes).expect("scratch write succeeds");
    }

    fn text_of(tail: &mut LogTail) -> String {
        tail.read().map(|c| c.text).unwrap_or_default()
    }

    #[test]
    fn a_missing_file_reads_as_nothing() {
        let mut tail = LogTail::from_start(scratch("absent.log"));
        assert!(tail.read().is_none());
    }

    #[test]
    fn each_line_is_delivered_exactly_once() {
        let path = scratch("append.log");
        append(&path, b"one\n");
        let mut tail = LogTail::from_start(path.clone());
        assert_eq!(text_of(&mut tail), "one\n");
        assert!(tail.read().is_none());

        append(&path, b"two\n");
        assert_eq!(text_of(&mut tail), "two\n");
        assert!(tail.read().is_none());
    }

    #[test]
    fn a_character_split_across_two_reads_survives() {
        let path = scratch("split.log");
        let stoefler = "Stöfler\n".as_bytes();
        let cut = 2; // mid-way through the two-byte 'ö'
        append(&path, &stoefler[..cut + 1]);

        let mut tail = LogTail::from_start(path.clone());
        let first = text_of(&mut tail);
        append(&path, &stoefler[cut + 1..]);
        let second = text_of(&mut tail);

        assert_eq!(format!("{first}{second}"), "Stöfler\n");
    }

    #[test]
    fn a_byte_that_is_no_character_does_not_stall_the_cursor() {
        let path = scratch("invalid.log");
        append(&path, b"before\n\xffafter\n");

        let mut tail = LogTail::from_start(path.clone());
        let text = text_of(&mut tail);
        assert!(
            text.contains("before"),
            "text before the bad byte is delivered"
        );
        assert!(text.contains("after"), "text after it is delivered too");
        assert!(tail.read().is_none(), "the cursor moved past the bad byte");
    }

    #[test]
    fn truncating_the_file_replays_it_and_says_so() {
        let path = scratch("truncate.log");
        append(&path, b"old session\n");
        let mut tail = LogTail::from_start(path.clone());
        assert_eq!(text_of(&mut tail), "old session\n");

        std::fs::write(&path, b"new\n").expect("scratch file is writable");
        let chunk = tail.read().expect("a truncated file reads from its start");
        assert_eq!(chunk.text, "new\n");
        assert!(chunk.restarted);
    }

    /// A relaunch replaces the file rather than truncating it, and the new log
    /// can pass the old cursor before the next poll. Length alone misses that.
    #[test]
    fn replacing_the_file_with_a_longer_one_still_reads_as_a_restart() {
        let path = scratch("replace.log");
        append(&path, b"old\n");
        let mut tail = LogTail::from_start(path.clone());
        assert_eq!(text_of(&mut tail), "old\n");

        std::fs::rename(&path, path.with_extension("old")).expect("scratch file is movable");
        append(&path, b"a much longer new session log\n");

        let chunk = tail.read().expect("a replaced file reads from its start");
        assert_eq!(chunk.text, "a much longer new session log\n");
        assert!(chunk.restarted);
    }

    #[test]
    fn a_tail_that_opened_at_the_end_still_sees_a_replacement() {
        let path = scratch("end-replace.log");
        append(&path, b"old session\n");
        let mut tail = LogTail::from_end(path.clone());
        assert!(tail.read().is_none(), "nothing has been appended yet");

        std::fs::rename(&path, path.with_extension("old")).expect("scratch file is movable");
        append(&path, b"a much longer new session log\n");

        let chunk = tail.read().expect("a replaced file reads from its start");
        assert_eq!(chunk.text, "a much longer new session log\n");
        assert!(chunk.restarted);
    }

    /// The restart has to reach the reader even when the read that noticed it
    /// had nothing to hand over.
    #[test]
    fn a_restart_seen_on_an_empty_file_is_reported_with_the_next_chunk() {
        let path = scratch("empty-restart.log");
        append(&path, b"old\n");
        let mut tail = LogTail::from_start(path.clone());
        assert_eq!(text_of(&mut tail), "old\n");

        std::fs::write(&path, b"").expect("scratch file is writable");
        assert!(
            tail.read().is_none(),
            "an empty file has nothing to deliver"
        );

        append(&path, b"new\n");
        let chunk = tail.read().expect("the new file has text");
        assert_eq!(chunk.text, "new\n");
        assert!(chunk.restarted, "the restart must not be forgotten");
    }
    #[test]
    fn split_lines_are_delivered_without_inventing_newlines() {
        let path = scratch("split-line.log");
        append(&path, b"first\nsec");
        let mut tail = LogTail::from_start(path.clone());
        assert_eq!(text_of(&mut tail), "first\nsec");
        append(&path, b"ond\n");
        assert_eq!(text_of(&mut tail), "ond\n");
        assert!(tail.read().is_none());
    }

    #[test]
    fn failed_open_retains_cursor_and_partial_character() {
        let path = scratch("retry.log");
        append(&path, b"old\n\xc3");
        let mut tail = LogTail::from_start(path.clone());
        assert_eq!(text_of(&mut tail), "old\n");
        let moved = path.with_extension("old");
        std::fs::rename(&path, &moved).expect("scratch file is movable");
        assert!(tail.read().is_none());
        assert_eq!(tail.pos, 5);
        assert_eq!(tail.partial_char, b"\xc3");
        std::fs::rename(&moved, &path).expect("scratch file is movable");
        append(&path, b"\xb6\n");
        assert_eq!(text_of(&mut tail), "ö\n");
    }
}
