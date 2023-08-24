use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::storage::wal::segment::Segment;
use crate::storage::{get_segment_range, Options};

/// Write-Ahead Log (WAL) is a data structure used to sequentially store records
/// in a series of segments. It provides efficient write operations,
/// making it suitable for use cases like write-ahead logging.
pub struct WAL {
    /// The currently active segment where data is being written.
    active_segment: Segment,

    /// The ID of the currently active segment.
    active_segment_id: u64,

    /// The directory where the segment files are located.
    dir: PathBuf,

    /// Configuration options for the WAL instance.
    opts: Options,

    /// A flag indicating whether the WAL instance is closed or not.
    closed: bool,

    /// A read-write lock used to synchronize concurrent access to the WAL instance.
    mutex: RwLock<()>,
}

impl WAL {
    /// Opens or creates a new WAL instance associated with the specified directory and segment ID.
    ///
    /// This function prepares the WAL instance by creating the necessary directory,
    /// determining the active segment ID, and initializing the active segment.
    ///
    /// # Parameters
    ///
    /// - `dir`: The directory where segment files are located.
    /// - `opts`: Configuration options for the WAL instance.
    pub fn open(dir: &Path, opts: &Options) -> io::Result<Self> {
        // Ensure the options are valid
        opts.validate()?;

        // Ensure the directory exists with proper permissions
        Self::prepare_directory(&dir, &opts)?;

        // Determine the active segment ID
        let active_segment_id = Self::calculate_active_segment_id(&dir)?;

        // Open the active segment
        let active_segment = Segment::open(&dir, active_segment_id, &opts)?;

        Ok(Self {
            active_segment,
            active_segment_id,
            dir: dir.to_path_buf(),
            opts: opts.clone(),
            closed: false,
            mutex: RwLock::new(()),
        })
    }

    // Helper function to prepare the directory with proper permissions
    fn prepare_directory(dir: &Path, opts: &Options) -> io::Result<()> {
        fs::create_dir_all(&dir)?;

        if let Ok(metadata) = fs::metadata(&dir) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(opts.dir_mode.unwrap_or(0o750));
            fs::set_permissions(&dir, permissions)?;
        }

        Ok(())
    }

    // Helper function to calculate the active segment ID
    fn calculate_active_segment_id(dir: &Path) -> io::Result<u64> {
        let (_, last) = get_segment_range(&dir)?;
        Ok(if last > 0 { last + 1 } else { 0 })
    }

    /// Appends a record to the active segment.
    ///
    /// This function appends the record to the active segment. If the active segment is
    /// full, a new segment will be created and the record will be appended to it.
    ///
    /// The function returns a tuple containing the offset at which the record was appended
    /// and the number of bytes written.
    ///
    /// # Arguments
    ///
    /// * `rec` - A reference to the byte slice containing the record to be appended.
    ///
    /// # Returns
    ///
    /// A result containing the tuple `(offset, bytes_written)` or an `io::Error` in case of failure.
    ///
    /// # Errors
    ///
    /// This function may return an error if the active segment is closed, the provided record
    /// is empty, or any I/O error occurs during the appending process.
    pub fn append(&mut self, rec: &[u8]) -> io::Result<(u64, usize)> {
        if self.closed {
            return Err(io::Error::new(io::ErrorKind::Other, "Segment is closed"));
        }

        if rec.is_empty() {
            return Err(io::Error::new(io::ErrorKind::Other, "buf is empty"));
        }

        let _lock = self.mutex.write().unwrap();

        // Get options and initialize variables
        let opts = &self.opts;
        let mut n = 0usize;
        let mut available = 0;
        let mut offset = 0;

        // Calculate available space in the active segment
        available = opts.max_file_size - self.active_segment.offset();

        // If space is not available, create a new segment
        if rec.len() as u64 > available {
            // Rotate to a new segment

            // Sync and close the active segment
            self.active_segment.close()?;

            // Update the active segment id and create a new segment
            self.active_segment_id += 1;
            let new_segment = Segment::open(&self.dir, self.active_segment_id, &self.opts)?;
            self.active_segment = new_segment;

            // Calculate available space in the new segment
            available = opts.max_file_size;
        }

        let (off, _) = self.active_segment.append(&rec)?;
        offset = off + self.calculate_offset();

        Ok((offset, rec.len()))
    }

    // Helper function to calculate offset
    fn calculate_offset(&self) -> u64 {
        self.active_segment_id * self.opts.max_file_size
    }

    /// Reads data from the segment at the specified offset into the provided buffer.
    ///
    /// This function reads data from the segment's underlying storage starting at the specified
    /// offset and writes it into the provided buffer. The number of bytes read is returned.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to a byte slice that will hold the read data.
    /// * `off` - The offset from which to start reading data within the segment.
    ///
    /// # Returns
    ///
    /// A result containing the number of bytes read or an `io::Error` in case of failure.
    ///
    /// # Errors
    ///
    /// This function may return an error if the provided buffer is empty, or any I/O error occurs
    /// during the reading process.
    pub fn read_at(&self, buf: &mut [u8], off: u64) -> io::Result<usize> {
        if buf.is_empty() {
            return Err(io::Error::new(io::ErrorKind::Other, "Buffer is empty"));
        }

        let mut r = 0;
        while r < buf.len() {
            let offset = off + r as u64;
            let segment_id = off / self.opts.max_file_size;
            let read_offset = offset % self.opts.max_file_size;

            // Read data from the appropriate segment
            r += self.read_segment_data(&mut buf[r..], segment_id, read_offset)?;
        }

        Ok(r)
    }

    // Helper function to read data from the appropriate segment
    fn read_segment_data(
        &self,
        buf: &mut [u8],
        segment_id: u64,
        read_offset: u64,
    ) -> io::Result<usize> {
        if segment_id == self.active_segment_id {
            self.active_segment.read_at(buf, read_offset)
        } else {
            let segment = Segment::open(&self.dir, segment_id, &self.opts)?;
            segment.read_at(buf, read_offset)
        }
    }

    pub fn close(&mut self) -> io::Result<()> {
        let _lock = self.mutex.write().unwrap();
        self.active_segment.close()?;
        Ok(())
    }

    pub fn sync(&mut self) -> io::Result<()> {
        let _lock = self.mutex.write().unwrap();
        self.active_segment.sync()?;
        Ok(())
    }

    // Returns the current offset within the segment.
    pub fn offset(&self) -> u64 {
        let _lock = self.mutex.read().unwrap();
        self.active_segment.offset()
    }
}

impl Drop for WAL {
    /// Attempt to fsync data on drop, in case we're running without sync.
    fn drop(&mut self) {
        self.close().ok();
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::RECORD_HEADER_SIZE;

    use super::*;
    use tempdir::TempDir;

    fn create_temp_directory() -> TempDir {
        TempDir::new("test").unwrap()
    }

    #[test]
    fn test_append() {
        // Create a temporary directory
        let temp_dir = create_temp_directory();

        // Create aol options and open a aol file
        let opts = Options::default();
        let mut a = WAL::open(&temp_dir.path(), &opts).expect("should create aol");

        // Test initial offset
        let sz = a.offset();
        assert_eq!(0, sz);

        // Test appending an empty buffer
        let r = a.append(&[]);
        assert!(r.is_err());

        // Test appending a non-empty buffer
        let r = a.append(&[0, 1, 2, 3]);
        assert!(r.is_ok());
        assert_eq!(4, r.unwrap().1);

        // Test appending another buffer
        let r = a.append(&[4, 5, 6, 7, 8, 9, 10]);
        assert!(r.is_ok());
        assert_eq!(7, r.unwrap().1);

        // Validate offset after appending
        // 8 + 4 + 8 + 7 = 27
        assert_eq!(a.offset(), 27);

        // Test syncing segment
        let r = a.sync();
        assert!(r.is_ok());

        // Validate offset after syncing
        assert_eq!(a.offset(), 4096);

        // Test reading from segment
        let mut bs = vec![0; 12];
        let n = a.read_at(&mut bs, 0).expect("should read");
        assert_eq!(12, n);
        assert_eq!(&[0, 1, 2, 3].to_vec(), &bs[RECORD_HEADER_SIZE..]);

        // Test reading another portion of data from segment
        let mut bs = vec![0; 15];
        let n = a.read_at(&mut bs, 12).expect("should read");
        assert_eq!(15, n);
        assert_eq!(&[4, 5, 6, 7, 8, 9, 10].to_vec(), &bs[RECORD_HEADER_SIZE..]);

        // Test reading beyond segment's current size
        let mut bs = vec![0; 15];
        let r = a.read_at(&mut bs, 4097);
        assert!(r.is_err());

        // Test appending another buffer after syncing
        let r = a.append(&[11, 12, 13, 14]);
        assert!(r.is_ok());
        assert_eq!(4, r.unwrap().1);

        // Validate offset after appending
        // 4096 + 8 + 4 = 4108
        assert_eq!(a.offset(), 4108);

        // Test reading from segment after appending
        let mut bs = vec![0; 12];
        let n = a.read_at(&mut bs, 4096).expect("should read");
        assert_eq!(12, n);
        assert_eq!(&[11, 12, 13, 14].to_vec(), &bs[RECORD_HEADER_SIZE..]);

        // Test syncing segment again
        let r = a.sync();
        assert!(r.is_ok());

        // Validate offset after syncing again
        assert_eq!(a.offset(), 4096 * 2);

        // Test closing segment
        assert!(a.close().is_ok());

        // Cleanup: Drop the temp directory, which deletes its contents
        drop(temp_dir);
    }
}
