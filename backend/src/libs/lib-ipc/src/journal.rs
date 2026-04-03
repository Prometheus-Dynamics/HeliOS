use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::{
    archive,
    envelope::{TaggedDecodeError, TaggedEncode, TaggedEnvelope},
    types::JournalMetadata,
};
use chrono::{DateTime, Utc};

const HEADER_LEN: usize = 4;
const DEFAULT_MAX_JOURNAL_BYTES: u64 = 8 * 1024 * 1024;
const MIN_MAX_JOURNAL_BYTES: u64 = 64 * 1024;
const MAX_MAX_JOURNAL_BYTES: u64 = 256 * 1024 * 1024;

fn map_encode_error(err: archive::Result<Vec<u8>>) -> io::Result<Vec<u8>> {
    err.map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn map_decode_error(err: archive::Result<TaggedEnvelope>) -> io::Result<TaggedEnvelope> {
    err.map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn map_tagged_encode_error(err: crate::envelope::TaggedEncodeError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

fn map_tagged_decode_error(err: TaggedDecodeError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

#[derive(Debug, Clone)]
pub struct JournalEntry<T> {
    pub offset: u64,
    pub next_offset: u64,
    pub payload: T,
}

impl<T> JournalEntry<T> {
    #[must_use]
    pub fn len_bytes(&self) -> u64 {
        self.next_offset - self.offset
    }
}

fn system_time_to_timestamp(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

#[derive(Debug)]
pub struct Journal<T> {
    path: PathBuf,
    file: Mutex<BufWriter<File>>,
    options: JournalOptions,
    last_trimmed_at: Mutex<Option<DateTime<Utc>>>,
    _marker: PhantomData<T>,
}

#[derive(Debug, Clone)]
pub struct JournalOptions {
    pub sync_on_append: bool,
    pub buffer_capacity: usize,
    pub max_bytes: Option<u64>,
}

impl Default for JournalOptions {
    fn default() -> Self {
        Self { sync_on_append: false, buffer_capacity: 64 * 1024, max_bytes: journal_max_bytes_from_env().or(Some(DEFAULT_MAX_JOURNAL_BYTES)) }
    }
}

impl<T> Journal<T> {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_options(path, JournalOptions::default())
    }

    pub fn open_with_options(path: impl AsRef<Path>, options: JournalOptions) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).read(true).append(true).open(&path)?;
        let writer = BufWriter::with_capacity(options.buffer_capacity, file);
        Ok(Self { path, file: Mutex::new(writer), options, last_trimmed_at: Mutex::new(None), _marker: PhantomData })
    }

    pub fn append(&self, payload: &T) -> io::Result<JournalEntry<T>>
    where
        T: Clone + TaggedEncode,
    {
        let mut file = self.file.lock().expect("journal poisoned");
        let data = payload.encode_envelope().map_err(map_tagged_encode_error).and_then(|envelope| map_encode_error(archive::encode_to_vec(&envelope)))?;
        if data.len() > u32::MAX as usize {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "journal payload exceeds 4 GiB limit"));
        }

        let len = data.len() as u32;
        let entry_len = HEADER_LEN as u64 + len as u64;
        let mut offset = file.seek(SeekFrom::End(0))?;
        if self.options.max_bytes.is_some_and(|max_bytes| offset > 0 && offset.saturating_add(entry_len) > max_bytes) {
            reset_writer(&mut file, self.options.sync_on_append)?;
            *self.last_trimmed_at.lock().expect("journal poisoned") = Some(Utc::now());
            offset = 0;
        }

        file.write_all(&len.to_le_bytes())?;
        file.write_all(&data)?;
        file.flush()?;
        if self.options.sync_on_append {
            file.get_ref().sync_data()?;
        }

        let next_offset = offset + HEADER_LEN as u64 + len as u64;
        Ok(JournalEntry { offset, next_offset, payload: payload.clone() })
    }

    pub fn append_batch<I>(&self, payloads: I) -> io::Result<Vec<JournalEntry<T>>>
    where
        I: IntoIterator,
        I::Item: AsRef<T>,
        T: Clone + TaggedEncode,
    {
        let mut file = self.file.lock().expect("journal poisoned");
        let mut entries = Vec::new();
        let mut encoded = Vec::new();
        let mut batch_len = 0u64;

        for payload in payloads {
            let payload = payload.as_ref();
            let data = payload.encode_envelope().map_err(map_tagged_encode_error).and_then(|envelope| map_encode_error(archive::encode_to_vec(&envelope)))?;
            if data.len() > u32::MAX as usize {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "journal payload exceeds 4 GiB limit"));
            }
            batch_len = batch_len.saturating_add(HEADER_LEN as u64 + data.len() as u64);
            encoded.push((payload.clone(), data));
        }

        let mut offset = file.seek(SeekFrom::End(0))?;
        if self.options.max_bytes.is_some_and(|max_bytes| offset > 0 && offset.saturating_add(batch_len) > max_bytes) {
            reset_writer(&mut file, self.options.sync_on_append)?;
            *self.last_trimmed_at.lock().expect("journal poisoned") = Some(Utc::now());
            offset = 0;
        }

        for (payload, data) in encoded {
            let len = data.len() as u32;
            file.write_all(&len.to_le_bytes())?;
            file.write_all(&data)?;

            let next_offset = offset + HEADER_LEN as u64 + len as u64;
            entries.push(JournalEntry { offset, next_offset, payload });
            offset = next_offset;
        }

        file.flush()?;
        if self.options.sync_on_append {
            file.get_ref().sync_data()?;
        }

        Ok(entries)
    }

    pub fn sync(&self) -> io::Result<()> {
        let mut file = self.file.lock().expect("journal poisoned");
        file.flush()?;
        file.get_ref().sync_data()
    }

    pub fn truncate(&self, len: u64) -> io::Result<()> {
        let mut file = self.file.lock().expect("journal poisoned");
        file.flush()?;
        file.get_ref().set_len(len)?;
        file.get_ref().sync_data()?;
        *self.last_trimmed_at.lock().expect("journal poisoned") = Some(Utc::now());
        Ok(())
    }

    pub fn truncate_through(&self, entry: &JournalEntry<T>) -> io::Result<()> {
        // Important: we must not truncate the file while reading the tail from the same path,
        // otherwise the reader will observe EOF (the OS reflects the new file size immediately).
        //
        // To keep this streaming and correct, we write the suffix to a temp file, then atomically
        // replace the journal on disk and reopen the writer handle.
        let mut file = self.file.lock().expect("journal poisoned");
        file.flush()?;
        if self.options.sync_on_append {
            file.get_ref().sync_data()?;
        }

        let mut reader = File::open(&self.path)?;
        reader.seek(SeekFrom::Start(entry.next_offset))?;

        let parent = self.path.parent().ok_or_else(|| io::Error::other("journal path has no parent"))?;
        let stem = self.path.file_name().ok_or_else(|| io::Error::other("journal path has no filename"))?.to_string_lossy();
        let tmp_path = parent.join(format!(".{stem}.truncate.{}.tmp", uuid::Uuid::new_v4()));

        let mut tmp = OpenOptions::new().create(true).write(true).truncate(true).open(&tmp_path)?;
        io::copy(&mut reader, &mut tmp)?;
        tmp.flush()?;
        tmp.sync_data()?;
        drop(tmp);

        // Replace the file at the path. On Unix this is atomic.
        // If the target exists and the platform doesn't support overwriting rename,
        // remove the old file first.
        if std::fs::rename(&tmp_path, &self.path).is_err() {
            let _ = std::fs::remove_file(&self.path);
            std::fs::rename(&tmp_path, &self.path)?;
        }

        // Reopen the writer so future appends target the new file.
        let reopened = OpenOptions::new().create(true).read(true).append(true).open(&self.path)?;
        *file = BufWriter::with_capacity(self.options.buffer_capacity, reopened);

        *self.last_trimmed_at.lock().expect("journal poisoned") = Some(Utc::now());
        Ok(())
    }

    pub fn replay(&self) -> io::Result<Vec<JournalEntry<T>>>
    where
        T: TryFrom<TaggedEnvelope, Error = TaggedDecodeError>,
    {
        let mut file = File::open(&self.path)?;
        let mut entries = Vec::new();
        let mut offset = 0u64;

        loop {
            let mut header = [0u8; HEADER_LEN];
            let read = file.read(&mut header)?;
            if read == 0 {
                break;
            }
            if read < HEADER_LEN {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "partial journal entry header"));
            }

            let len = u32::from_le_bytes(header) as usize;
            let mut data = vec![0u8; len];
            file.read_exact(&mut data)?;
            let envelope = map_decode_error(archive::decode_from_slice(&data))?;
            let payload = T::try_from(envelope).map_err(map_tagged_decode_error)?;
            let next_offset = offset + HEADER_LEN as u64 + len as u64;
            entries.push(JournalEntry { offset, next_offset, payload });
            offset = next_offset;
        }

        Ok(entries)
    }

    pub fn metadata(&self) -> io::Result<JournalMetadata> {
        let meta = std::fs::metadata(&self.path)?;
        let created_at = meta.created().ok().map(system_time_to_timestamp).unwrap_or_else(Utc::now);
        let mut entries = 0u64;
        let mut reader = File::open(&self.path)?;
        loop {
            let mut header = [0u8; HEADER_LEN];
            let read = reader.read(&mut header)?;
            if read == 0 {
                break;
            }
            if read < HEADER_LEN {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "partial journal entry header"));
            }
            let len = u32::from_le_bytes(header) as u64;
            reader.seek(SeekFrom::Current(len as i64))?;
            entries += 1;
        }
        let last_trimmed_at = self.last_trimmed_at.lock().expect("journal poisoned").as_ref().copied();
        Ok(JournalMetadata { file_name: self.path.display().to_string(), created_at, last_trimmed_at, entries })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn journal_max_bytes_from_env() -> Option<u64> {
    std::env::var("HELIOS_IPC_JOURNAL_MAX_BYTES").ok().and_then(|raw| raw.trim().parse::<u64>().ok()).map(|value| value.clamp(MIN_MAX_JOURNAL_BYTES, MAX_MAX_JOURNAL_BYTES))
}

fn reset_writer(file: &mut BufWriter<File>, sync_on_append: bool) -> io::Result<()> {
    file.flush()?;
    file.get_ref().set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    if sync_on_append {
        file.get_ref().sync_data()?;
    }
    Ok(())
}

pub type JournalWriter<T> = Journal<T>;
pub type JournalReader<T> = Journal<T>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive;
    use crate::types::CommandId;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;
    use uuid::Uuid;

    macro_rules! error {
        ($($tt:tt)*) => {};
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    enum TestCommand {
        ApplyInput { command_id: CommandId, pipeline_id: Uuid, port: String, value: i64 },
        Heartbeat { command_id: CommandId, sequence: u64 },
    }

    #[repr(u16)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestCommandKind {
        ApplyInput = 0,
        Heartbeat = 1,
    }

    impl TestCommandKind {
        const fn to_u16(self) -> u16 {
            self as u16
        }

        fn from_u16(value: u16) -> Option<Self> {
            match value {
                0 => Some(Self::ApplyInput),
                1 => Some(Self::Heartbeat),
                _ => None,
            }
        }
    }

    const _: () = {
        crate::tagged_enum! {
            impl crate::journal::tests::TestCommand => crate::journal::tests::TestCommandKind {
                struct ApplyInput {
                    command_id: CommandId => with_serde,
                    pipeline_id: Uuid => with_serde,
                    port: String,
                    value: i64
                },
                struct Heartbeat {
                    command_id: CommandId => with_serde,
                    sequence: u64
                },
            }
        }
    };

    fn sample_apply_command() -> (CommandId, TestCommand) {
        let command_id = CommandId::new();
        let command = TestCommand::ApplyInput { command_id, pipeline_id: Uuid::new_v4(), port: "exposure".to_string(), value: 42 };
        (command_id, command)
    }

    #[test]
    fn test_command_roundtrip() {
        let (_, command) = sample_apply_command();
        let envelope = command.encode_envelope().expect("serialize engine command");
        let decoded = TestCommand::try_from(envelope).expect("deserialize engine command");
        assert!(matches!(decoded, TestCommand::ApplyInput { .. }));
    }

    #[test]
    fn append_and_replay_round_trip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("engine.log");
        let journal: Journal<TestCommand> = Journal::open(&path).expect("open journal");

        let (first_id, first_cmd) = sample_apply_command();
        let (second_id, second_cmd) = sample_apply_command();

        journal.append(&first_cmd).expect("append first");
        journal.append(&second_cmd).expect("append second");

        let entries = journal.replay().expect("replay");
        assert_eq!(entries.len(), 2);

        match &entries[0].payload {
            TestCommand::ApplyInput { command_id, .. } => assert_eq!(*command_id, first_id),
            other => panic!("unexpected payload: {other:?}"),
        }
        match &entries[1].payload {
            TestCommand::ApplyInput { command_id, .. } => assert_eq!(*command_id, second_id),
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn truncate_removes_prefix() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("engine.truncate");
        let journal: Journal<TestCommand> = Journal::open(&path).expect("open");

        let (first_id, first_cmd) = sample_apply_command();
        let first_entry = journal.append(&first_cmd).expect("append 1");
        let (second_id, second_cmd) = sample_apply_command();
        journal.append(&second_cmd).expect("append 2");

        journal.truncate_through(&first_entry).expect("truncate");

        let entries = journal.replay().expect("replay");
        assert_eq!(entries.len(), 1);
        match &entries[0].payload {
            TestCommand::ApplyInput { command_id, .. } => {
                assert_eq!(*command_id, second_id);
                assert_ne!(*command_id, first_id);
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn heavy_append_load() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("engine.load");
        let journal: Journal<TestCommand> = Journal::open(&path).expect("open");

        for _ in 0..1_000 {
            let (_, cmd) = sample_apply_command();
            journal.append(&cmd).expect("append");
        }

        let entries = journal.replay().expect("replay");
        assert_eq!(entries.len(), 1_000);
    }

    #[test]
    fn append_rolls_journal_when_max_bytes_is_exceeded() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("engine.capped");
        let journal: Journal<TestCommand> = Journal::open_with_options(&path, JournalOptions { sync_on_append: false, buffer_capacity: 1024, max_bytes: Some(512) }).expect("open");

        let mut last_command = None;
        for _ in 0..32 {
            let (_, command) = sample_apply_command();
            journal.append(&command).expect("append");
            last_command = Some(command);
        }

        let file_len = std::fs::metadata(&path).expect("metadata").len();
        assert!(file_len <= 512);

        let entries = journal.replay().expect("replay");
        assert!(!entries.is_empty());
        assert!(entries.len() < 32);
        assert_eq!(entries.last().expect("last entry").payload, last_command.expect("last command"));
    }

    #[test]
    fn command_id_roundtrip() {
        let command_id = CommandId::new();
        let bytes = archive::encode_to_vec(&command_id).expect("serialize command id");
        let decoded: CommandId = archive::decode_from_slice(&bytes).expect("deserialize command id");
        assert_eq!(decoded, command_id);
    }

    #[test]
    fn uuid_roundtrip() {
        let uuid = Uuid::new_v4().as_u128();
        let bytes = archive::encode_to_vec(&uuid).expect("serialize uuid");
        let decoded: u128 = archive::decode_from_slice(&bytes).expect("deserialize uuid");
        assert_eq!(decoded, uuid);
    }
}
