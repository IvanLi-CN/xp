use std::{
    fs,
    io::{BufWriter, Write},
    path::Path,
};

use super::PersistedTelemetry;

pub(super) fn persist(path: &Path, state: &PersistedTelemetry) -> anyhow::Result<()> {
    let parent = path.parent().expect("mesh telemetry path has a parent");
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    {
        let file = fs::File::create(&temporary)?;
        let mut writer = BufWriter::with_capacity(8 * 1024, file);
        write_snapshot(&mut writer, state)?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|error| error.into_error())?;
        file.sync_all()?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn write_snapshot(mut destination: impl Write, state: &PersistedTelemetry) -> anyhow::Result<()> {
    serde_json::to_writer_pretty(&mut destination, state)?;
    destination.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    struct FailedFlush(Vec<u8>);

    impl Write for FailedFlush {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("injected snapshot flush failure"))
        }
    }

    #[test]
    fn snapshot_flush_failure_is_reported_before_durable_completion() {
        let mut destination = FailedFlush(Vec::new());
        let result = write_snapshot(&mut destination, &PersistedTelemetry::default());
        let expected = concat!(
            "{\n  \"schema_version\": 1,\n  \"revision\": 0,\n",
            "  \"peers\": {},\n  \"events\": []\n}"
        );
        assert_eq!(destination.0, expected.as_bytes());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected snapshot flush failure")
        );
    }

    struct FailedWrite;

    impl Write for FailedWrite {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("injected snapshot write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn snapshot_write_failure_is_reported_before_durable_completion() {
        let result = write_snapshot(FailedWrite, &PersistedTelemetry::default());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected snapshot write failure")
        );
    }
}
