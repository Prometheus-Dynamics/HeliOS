use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};
use zip::write::SimpleFileOptions;

pub(super) fn build_media_archive_file(entries: Vec<(String, PathBuf)>) -> Result<std::fs::File, String> {
    let mut archive_file = tempfile::tempfile().map_err(|err| format!("failed to allocate temporary archive: {err}"))?;
    {
        let mut writer = zip::ZipWriter::new(archive_file);
        let mut copy_buffer = [0u8; 64 * 1024];
        for (name, path) in entries {
            let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(name.as_str(), options).map_err(|err| format!("failed to start archive entry for {name}: {err}"))?;
            let mut input = std::fs::File::open(&path).map_err(|err| format!("failed to open media file {name}: {err}"))?;
            loop {
                let read = input.read(&mut copy_buffer).map_err(|err| format!("failed to read media file {name}: {err}"))?;
                if read == 0 {
                    break;
                }
                writer.write_all(&copy_buffer[..read]).map_err(|err| format!("failed to write archive entry for {name}: {err}"))?;
            }
        }
        archive_file = writer.finish().map_err(|err| format!("failed to finalize archive: {err}"))?;
    }
    archive_file.seek(SeekFrom::Start(0)).map_err(|err| format!("failed to rewind archive: {err}"))?;
    Ok(archive_file)
}
