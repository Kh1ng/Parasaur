use super::ControllerEvent;
use anyhow::{Context, Result};
use fs2::FileExt;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

/// Visit complete events up to the snapshot boundary. Return only an
/// unterminated JSON fragment or an unterminated run of zero-filled bytes.
/// A newline commits a record, so even an EOF parse error there is corruption.
pub(super) fn scan(
    file: &File,
    len: u64,
    path: &Path,
    mut visit: impl FnMut(ControllerEvent),
) -> Result<Option<(u64, Vec<u8>)>> {
    let mut reader = BufReader::new(file.take(len));
    let mut offset = 0;
    let mut line = Vec::new();
    for number in 1usize.. {
        line.clear();
        let size = reader
            .read_until(b'\n', &mut line)
            .with_context(|| format!("reading {}", path.display()))?;
        if size == 0 {
            break;
        }
        if !std::str::from_utf8(&line).is_ok_and(|s| s.trim().is_empty()) {
            match serde_json::from_slice(&line) {
                Ok(event) => visit(event),
                Err(error)
                    if !line.ends_with(b"\n")
                        && (error.is_eof() || line.iter().all(|&b| b == 0)) =>
                {
                    return Ok(Some((offset, line)))
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("parsing event {number} from {}", path.display()))
                }
            }
        }
        offset += size as u64;
    }
    Ok(None)
}

/// Called with the existing writer lock. A healthy journal costs one final
/// byte check. Validate the entire prefix before preserving or changing a tail.
pub(super) fn prepare_append(file: &mut File, path: &Path) -> Result<()> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(());
    }

    FileExt::lock_exclusive(file)?;
    file.rewind()?;
    if let Some((offset, bytes)) = scan(file, len, path, |_| {})? {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut backup = tempfile::Builder::new()
            .prefix("events-torn-tail-")
            .tempfile_in(parent)?;
        backup.write_all(&bytes)?;
        backup.as_file().sync_all()?;
        let (_, saved) = backup.keep()?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        file.set_len(offset)?;
        file.sync_all()?;
        eprintln!(
            "Recovered {} at byte {offset}; saved {} trailing bytes in {}",
            path.display(),
            bytes.len(),
            saved.display()
        );
    } else {
        // A complete final JSON value without its delimiter is still an event.
        file.write_all(b"\n")?;
    }
    Ok(())
}
