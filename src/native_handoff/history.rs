//! Ordered ledger chunks retain complete rows without making history a single
//! in-memory value. A manifest is published only after every chunk is durable.
use super::{Blob, Result};
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, path::Path};

pub(super) fn name(index: u32) -> String {
    format!("history-{index:04}.jsonl")
}

pub(super) struct Writer<'a> {
    root: &'a Path,
    chunk_limit: u64,
    max_chunks: u32,
    current: Option<tempfile::NamedTempFile>,
    bytes: u64,
    digest: Sha256,
    chunks: BTreeMap<String, Blob>,
}

impl<'a> Writer<'a> {
    pub(super) fn new(root: &'a Path, chunk_limit: u64, max_chunks: u32) -> Self {
        Self {
            root,
            chunk_limit,
            max_chunks,
            current: None,
            bytes: 0,
            digest: Sha256::new(),
            chunks: BTreeMap::new(),
        }
    }

    /// Keep a ledger row intact. Oversized histories fail without publishing a
    /// ready manifest; they never silently become a partial or empty history.
    pub(super) fn append(&mut self, row: &[u8]) -> Result<()> {
        let size = row.len() as u64 + 1;
        ensure!(
            size <= self.chunk_limit,
            "Native history row exceeds its bounded chunk limit"
        );
        if self.bytes + size > self.chunk_limit {
            self.finish_chunk()?;
        }
        if self.current.is_none() {
            ensure!(self.chunks.len() < self.max_chunks as usize,
                "Native profile history exceeds its bounded chunk count; retain history for reconciliation");
            self.current = Some(tempfile::NamedTempFile::new_in(self.root)?);
        }
        let file = self.current.as_mut().unwrap();
        file.write_all(row)?;
        file.write_all(b"\n")?;
        self.digest.update(row);
        self.digest.update(b"\n");
        self.bytes += size;
        Ok(())
    }

    fn finish_chunk(&mut self) -> Result<()> {
        let file = self
            .current
            .take()
            .context("Native history chunk is missing")?;
        file.as_file().sync_all()?;
        let name = name(self.chunks.len() as u32);
        file.persist_noclobber(self.root.join(&name))
            .context("Native history chunk already exists or cannot be committed")?;
        self.chunks.insert(
            name,
            Blob {
                bytes: self.bytes,
                sha256: format!("{:x}", std::mem::take(&mut self.digest).finalize()),
            },
        );
        self.bytes = 0;
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<BTreeMap<String, Blob>> {
        if self.current.is_none() && self.chunks.is_empty() {
            self.current = Some(tempfile::NamedTempFile::new_in(self.root)?);
        }
        if self.current.is_some() {
            self.finish_chunk()?;
        }
        Ok(self.chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_preserve_order_bytes_and_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let mut writer = Writer::new(root.path(), 8, 3);
        for row in [b"one".as_slice(), b"two", b"three", b"six"] {
            writer.append(row).unwrap();
        }
        let chunks = writer.finish().unwrap();
        assert_eq!(chunks.len(), 3);
        let mut output = Vec::new();
        for (name, blob) in chunks {
            let path = root.path().join(name);
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
            super::super::copy_blob(&path, Some(&blob), 8, &mut output).unwrap();
        }
        assert_eq!(output, b"one\ntwo\nthree\nsix\n");
    }

    #[test]
    fn history_limits_refuse_overflow_without_overwriting_prior_chunks() {
        let root = tempfile::tempdir().unwrap();
        let mut writer = Writer::new(root.path(), 4, 2);
        writer.append(b"one").unwrap();
        writer.append(b"two").unwrap();
        assert!(writer.append(b"six").is_err());
        assert_eq!(std::fs::read(root.path().join(name(0))).unwrap(), b"one\n");
        assert_eq!(std::fs::read(root.path().join(name(1))).unwrap(), b"two\n");
        let mut writer = Writer::new(root.path(), 4, 2);
        assert!(writer.append(b"oversized").is_err());
        writer.append(b"six").unwrap();
        assert!(writer.finish().is_err());
        assert_eq!(std::fs::read(root.path().join(name(0))).unwrap(), b"one\n");
    }
}
