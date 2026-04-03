use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::archive;
use crate::types::{CommandId, JournalMetadata, RequestIdentity};
use crate::wire::{SCHEMA_VERSION, ServiceKind};
use chrono::{DateTime, Utc};
const HEADER_LEN: usize = 24;
const DEFAULT_MAX_JOURNAL_BYTES: u64 = 8 * 1024 * 1024;
const MIN_MAX_JOURNAL_BYTES: u64 = 64 * 1024;
const MAX_MAX_JOURNAL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct JournalEntry<T> {
    pub offset: u64,
    pub next_offset: u64,
    pub request_id: CommandId,
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
    service: ServiceKind,
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
    pub fn open(path: impl AsRef<Path>, service: ServiceKind) -> io::Result<Self> {
        Self::open_with_options(path, service, JournalOptions::default())
    }

    pub fn open_with_options(path: impl AsRef<Path>, service: ServiceKind, options: JournalOptions) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).read(true).append(true).open(&path)?;
        let writer = BufWriter::with_capacity(options.buffer_capacity, file);
        Ok(Self { path, file: Mutex::new(writer), options, service, last_trimmed_at: Mutex::new(None), _marker: PhantomData })
    }

    pub fn append(&self, payload: &T) -> io::Result<JournalEntry<T>>
    where
        T: Clone + archive::TransportEncode + RequestIdentity,
    {
        let request_id = payload.request_id();
        let encoded = archive::encode_to_vec(payload).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
        self.append_encoded(payload.clone(), request_id, encoded)
    }

    fn append_encoded(&self, payload: T, request_id: CommandId, encoded: Vec<u8>) -> io::Result<JournalEntry<T>> {
        if encoded.len() > u32::MAX as usize {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "journal payload exceeds 4 GiB limit"));
        }

        let mut file = self.file.lock().expect("journal poisoned");
        let entry_len = HEADER_LEN as u64 + encoded.len() as u64;
        let mut offset = file.seek(SeekFrom::End(0))?;
        if self.options.max_bytes.is_some_and(|max_bytes| offset > 0 && offset.saturating_add(entry_len) > max_bytes) {
            reset_writer(&mut file, self.options.sync_on_append)?;
            *self.last_trimmed_at.lock().expect("journal poisoned") = Some(Utc::now());
            offset = 0;
        }

        let mut header = [0u8; HEADER_LEN];
        header[0..2].copy_from_slice(&SCHEMA_VERSION.to_le_bytes());
        header[2..4].copy_from_slice(&self.service.to_u16().to_le_bytes());
        header[4..20].copy_from_slice(&request_id.as_uuid().as_u128().to_le_bytes());
        header[20..24].copy_from_slice(&(encoded.len() as u32).to_le_bytes());

        file.write_all(&header)?;
        file.write_all(&encoded)?;
        file.flush()?;
        if self.options.sync_on_append {
            file.get_ref().sync_data()?;
        }

        let next_offset = offset + entry_len;
        Ok(JournalEntry { offset, next_offset, request_id, payload })
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

        if std::fs::rename(&tmp_path, &self.path).is_err() {
            let _ = std::fs::remove_file(&self.path);
            std::fs::rename(&tmp_path, &self.path)?;
        }

        let reopened = OpenOptions::new().create(true).read(true).append(true).open(&self.path)?;
        *file = BufWriter::with_capacity(self.options.buffer_capacity, reopened);
        *self.last_trimmed_at.lock().expect("journal poisoned") = Some(Utc::now());
        Ok(())
    }

    pub fn replay(&self) -> io::Result<Vec<JournalEntry<T>>>
    where
        T: rkyv::Archive + RequestIdentity,
        T::Archived: for<'a> rkyv::bytecheck::CheckBytes<archive::DecodeValidator<'a>> + rkyv::Deserialize<T, archive::DecodeStrategy>,
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

            let version = u16::from_le_bytes(header[0..2].try_into().expect("journal version slice"));
            if version != SCHEMA_VERSION {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unsupported IPC journal schema_version {version}; expected {SCHEMA_VERSION}")));
            }
            let service = ServiceKind::from_u16(u16::from_le_bytes(header[2..4].try_into().expect("journal service slice")))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown IPC journal service kind"))?;
            if service != self.service {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("journal service mismatch: expected {:?}, found {:?}", self.service, service)));
            }

            let request_id = CommandId::from_uuid(uuid::Uuid::from_u128(u128::from_le_bytes(header[4..20].try_into().expect("journal request id slice"))));
            let len = u32::from_le_bytes(header[20..24].try_into().expect("journal payload len slice")) as usize;
            let mut payload_bytes = vec![0u8; len];
            file.read_exact(&mut payload_bytes)?;
            let payload = archive::decode_from_slice(&payload_bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

            let next_offset = offset + HEADER_LEN as u64 + len as u64;
            entries.push(JournalEntry { offset, next_offset, request_id, payload });
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
            let len = u32::from_le_bytes(header[20..24].try_into().expect("journal payload len slice")) as u64;
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
    use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
    enum TestCommand {
        ApplyInput { command_id: CommandId, pipeline_id: Uuid, port: String, value: i64 },
        Heartbeat { command_id: CommandId, sequence: u64 },
    }

    impl RequestIdentity for TestCommand {
        fn request_id(&self) -> CommandId {
            match self {
                Self::ApplyInput { command_id, .. } | Self::Heartbeat { command_id, .. } => *command_id,
            }
        }
    }

    #[test]
    fn replay_roundtrips_entries() {
        let dir = tempdir().expect("tempdir");
        let journal = Journal::<TestCommand>::open(dir.path().join("test.journal"), ServiceKind::Test).expect("journal");
        let first = TestCommand::ApplyInput { command_id: CommandId::new(), pipeline_id: Uuid::new_v4(), port: "input".to_string(), value: 7 };
        let second = TestCommand::Heartbeat { command_id: CommandId::new(), sequence: 3 };

        journal.append(&first).expect("append first");
        journal.append(&second).expect("append second");

        let entries = journal.replay().expect("replay");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].payload, first);
        assert_eq!(entries[1].payload, second);
    }

    #[test]
    fn truncate_through_keeps_tail_entries() {
        let dir = tempdir().expect("tempdir");
        let journal = Journal::<TestCommand>::open(dir.path().join("test.journal"), ServiceKind::Test).expect("journal");
        let first = TestCommand::Heartbeat { command_id: CommandId::new(), sequence: 1 };
        let second = TestCommand::Heartbeat { command_id: CommandId::new(), sequence: 2 };

        let first_entry = journal.append(&first).expect("append first");
        journal.append(&second).expect("append second");
        journal.truncate_through(&first_entry).expect("truncate");

        let entries = journal.replay().expect("replay");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload, second);
    }
}
