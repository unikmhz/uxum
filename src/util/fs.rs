use std::{io, path::Path};

use tokio::{fs, io::AsyncReadExt};

/// Read whole file into new buffer.
///
/// # Errors
///
/// Returns `Err` if there was an I/O error while opening or reading the file.
pub(crate) async fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>, io::Error> {
    let mut file = fs::OpenOptions::new().read(true).open(path).await?;
    let meta = file.metadata().await?;
    let mut buf = Vec::with_capacity(meta.len() as usize);
    file.read_to_end(&mut buf).await?;
    Ok(buf)
}
