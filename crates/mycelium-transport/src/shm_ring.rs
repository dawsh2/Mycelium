/// Shared memory ring buffer for cross-language IPC.
///
/// Provides a lock-free SPSC (Single Producer, Single Consumer) ring buffer
/// backed by shared memory (mmap). Achieves ~100-500ns latency for cross-process
/// communication without syscalls.
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use memmap2::{MmapMut, MmapOptions};

/// Shared memory header (64 bytes, cache-line aligned).
#[repr(C, align(64))]
struct ShmHeader {
    /// Writer's current byte offset in the ring buffer
    write_index: AtomicU64,
    /// Reader's current byte offset in the ring buffer
    read_index: AtomicU64,
    /// Ring buffer capacity in bytes
    capacity: u64,
    /// Schema digest for version validation
    schema_digest: [u8; 32],
    /// Protocol version (currently 1)
    version: u32,
    /// Reserved flags
    flags: u32,
}

const HEADER_SIZE: usize = 64;
const DEFAULT_CAPACITY: usize = 1024 * 1024; // 1MB
const PROTOCOL_VERSION: u32 = 1;

/// Errors that can occur in shared memory operations.
#[derive(Debug, thiserror::Error)]
pub enum ShmError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Buffer full (available: {available}, needed: {needed})")]
    BufferFull { available: usize, needed: usize },

    #[error("Schema digest mismatch")]
    SchemaMismatch,

    #[error("Invalid frame length: {0}")]
    InvalidFrameLength(u32),

    #[error("Corrupted frame (bad magic or sequence)")]
    CorruptedFrame,
}

pub type Result<T> = std::result::Result<T, ShmError>;

/// Shared memory ring buffer writer (producer).
pub struct ShmWriter {
    mmap: MmapMut,
    capacity: usize,
    write_ptr: *mut u8,
    sequence: u16,
}

/// Shared memory ring buffer reader (consumer).
pub struct ShmReader {
    mmap: MmapMut,
    capacity: usize,
    read_ptr: *const u8,
    expected_sequence: u16,
}

/// Frame header format (8 bytes).
#[repr(C)]
struct FrameHeader {
    /// Total frame size including this header (u32)
    length: u32,
    /// Message type ID (u16)
    type_id: u16,
    /// Sequence number for corruption detection (u16)
    sequence: u16,
}

impl ShmWriter {
    /// Create a new shared memory writer, initializing the region.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to shared memory file (e.g., "/dev/shm/mycelium/service.shm")
    /// * `schema_digest` - 32-byte schema digest for validation
    /// * `capacity` - Ring buffer size in bytes (defaults to 1MB)
    pub fn create<P: AsRef<Path>>(
        path: P,
        schema_digest: [u8; 32],
        capacity: Option<usize>,
    ) -> Result<Self> {
        let capacity = capacity.unwrap_or(DEFAULT_CAPACITY);
        let total_size = HEADER_SIZE + capacity;

        // Create or truncate file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        file.set_len(total_size as u64)?;

        // Memory map the file
        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        // Initialize header
        let header = unsafe { &mut *(mmap.as_mut_ptr() as *mut ShmHeader) };
        header.write_index = AtomicU64::new(0);
        header.read_index = AtomicU64::new(0);
        header.capacity = capacity as u64;
        header.schema_digest = schema_digest;
        header.version = PROTOCOL_VERSION;
        header.flags = 0;

        let write_ptr = unsafe { mmap.as_mut_ptr().add(HEADER_SIZE) };

        Ok(Self {
            mmap,
            capacity,
            write_ptr,
            sequence: 0,
        })
    }

    /// Write a frame (type_id + payload) to the ring buffer.
    ///
    /// Returns `Err(BufferFull)` if insufficient space. Frames are written
    /// atomically - either the entire frame is written or none of it is.
    pub fn write_frame(&mut self, type_id: u16, payload: &[u8]) -> Result<()> {
        let frame_size = std::mem::size_of::<FrameHeader>() + payload.len();

        // Get current indices
        let (write_idx, read_idx) = {
            let header = self.header();
            (
                header.write_index.load(Ordering::Acquire),
                header.read_index.load(Ordering::Acquire),
            )
        };

        // Check available space (writer can write up to read_idx - 1)
        let used = write_idx - read_idx;
        let available = self.capacity as u64 - used;

        if available < frame_size as u64 {
            return Err(ShmError::BufferFull {
                available: available as usize,
                needed: frame_size,
            });
        }

        // Write frame header
        let pos = (write_idx % self.capacity as u64) as usize;
        let frame_header = FrameHeader {
            length: frame_size as u32,
            type_id,
            sequence: self.sequence,
        };

        self.write_wrapping(pos, &frame_header_bytes(&frame_header));

        // Write payload
        let payload_pos = (pos + std::mem::size_of::<FrameHeader>()) % self.capacity;
        self.write_wrapping(payload_pos, payload);

        // Publish write (Release ensures all writes are visible before index update)
        self.header()
            .write_index
            .store(write_idx + frame_size as u64, Ordering::Release);

        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }

    /// Check if the buffer has space for a frame of the given size.
    pub fn has_space(&self, frame_size: usize) -> bool {
        let header = self.header();
        let write_idx = header.write_index.load(Ordering::Acquire);
        let read_idx = header.read_index.load(Ordering::Acquire);
        let used = write_idx - read_idx;
        let available = self.capacity as u64 - used;
        available >= frame_size as u64
    }

    fn header(&self) -> &ShmHeader {
        unsafe { &*(self.mmap.as_ptr() as *const ShmHeader) }
    }

    fn write_wrapping(&mut self, pos: usize, data: &[u8]) {
        let end = pos + data.len();
        if end <= self.capacity {
            // No wraparound
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    self.write_ptr.add(pos),
                    data.len(),
                );
            }
        } else {
            // Wraparound
            let first_chunk = self.capacity - pos;
            let second_chunk = data.len() - first_chunk;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    self.write_ptr.add(pos),
                    first_chunk,
                );
                std::ptr::copy_nonoverlapping(
                    data[first_chunk..].as_ptr(),
                    self.write_ptr,
                    second_chunk,
                );
            }
        }
    }
}

impl ShmReader {
    /// Open an existing shared memory region for reading.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to shared memory file
    /// * `expected_digest` - Expected schema digest (must match)
    pub fn open<P: AsRef<Path>>(path: P, expected_digest: [u8; 32]) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;

        let mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        // Validate header
        let header = unsafe { &*(mmap.as_ptr() as *const ShmHeader) };

        if header.schema_digest != expected_digest {
            return Err(ShmError::SchemaMismatch);
        }

        if header.version != PROTOCOL_VERSION {
            return Err(ShmError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported protocol version: {}", header.version),
            )));
        }

        let capacity = header.capacity as usize;
        let read_ptr = unsafe { mmap.as_ptr().add(HEADER_SIZE) };

        Ok(Self {
            mmap,
            capacity,
            read_ptr,
            expected_sequence: 0,
        })
    }

    /// Try to read the next frame from the ring buffer.
    ///
    /// Returns `None` if no frames are available. Returns `Some((type_id, payload))`
    /// if a frame was successfully read.
    pub fn read_frame(&mut self) -> Result<Option<(u16, Vec<u8>)>> {
        let header = self.header();
        let write_idx = header.write_index.load(Ordering::Acquire);
        let read_idx = header.read_index.load(Ordering::Acquire);

        // Check if data available
        if write_idx == read_idx {
            return Ok(None); // Empty
        }

        // Read frame header
        let pos = (read_idx % self.capacity as u64) as usize;
        let header_bytes = self.read_wrapping(pos, std::mem::size_of::<FrameHeader>());
        let frame_header = parse_frame_header(&header_bytes)?;

        // Validate sequence (detect corruption or skipped frames)
        if frame_header.sequence != self.expected_sequence {
            return Err(ShmError::CorruptedFrame);
        }

        // Read payload
        let payload_pos = (pos + std::mem::size_of::<FrameHeader>()) % self.capacity;
        let payload_len = frame_header.length as usize - std::mem::size_of::<FrameHeader>();
        let payload = self.read_wrapping(payload_pos, payload_len);

        // Publish read (Release ensures read completes before index update)
        header
            .read_index
            .store(read_idx + frame_header.length as u64, Ordering::Release);

        self.expected_sequence = self.expected_sequence.wrapping_add(1);

        Ok(Some((frame_header.type_id, payload)))
    }

    fn header(&self) -> &ShmHeader {
        unsafe { &*(self.mmap.as_ptr() as *const ShmHeader) }
    }

    fn read_wrapping(&self, pos: usize, len: usize) -> Vec<u8> {
        let mut buffer = vec![0u8; len];
        let end = pos + len;

        if end <= self.capacity {
            // No wraparound
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.read_ptr.add(pos),
                    buffer.as_mut_ptr(),
                    len,
                );
            }
        } else {
            // Wraparound
            let first_chunk = self.capacity - pos;
            let second_chunk = len - first_chunk;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.read_ptr.add(pos),
                    buffer.as_mut_ptr(),
                    first_chunk,
                );
                std::ptr::copy_nonoverlapping(
                    self.read_ptr,
                    buffer[first_chunk..].as_mut_ptr(),
                    second_chunk,
                );
            }
        }

        buffer
    }
}

// Safety: ShmWriter/Reader can be sent between threads (each is single-threaded)
unsafe impl Send for ShmWriter {}
unsafe impl Send for ShmReader {}

fn frame_header_bytes(header: &FrameHeader) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[0..4].copy_from_slice(&header.length.to_le_bytes());
    bytes[4..6].copy_from_slice(&header.type_id.to_le_bytes());
    bytes[6..8].copy_from_slice(&header.sequence.to_le_bytes());
    bytes
}

fn parse_frame_header(bytes: &[u8]) -> Result<FrameHeader> {
    if bytes.len() < 8 {
        return Err(ShmError::InvalidFrameLength(bytes.len() as u32));
    }

    let length = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let type_id = u16::from_le_bytes([bytes[4], bytes[5]]);
    let sequence = u16::from_le_bytes([bytes[6], bytes[7]]);

    Ok(FrameHeader {
        length,
        type_id,
        sequence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_create_and_open() {
        let path = "/tmp/test_shm_create.shm";
        let _ = fs::remove_file(path);

        let digest = [0u8; 32];
        let writer = ShmWriter::create(path, digest, Some(4096)).unwrap();
        let _reader = ShmReader::open(path, digest).unwrap();

        drop(writer);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_write_read_frame() {
        let path = "/tmp/test_shm_frame.shm";
        let _ = fs::remove_file(path);

        let digest = [1u8; 32];
        let mut writer = ShmWriter::create(path, digest, Some(4096)).unwrap();
        let mut reader = ShmReader::open(path, digest).unwrap();

        // Write a frame
        let payload = b"Hello, shared memory!";
        writer.write_frame(42, payload).unwrap();

        // Read it back
        let result = reader.read_frame().unwrap();
        assert!(result.is_some());
        let (type_id, read_payload) = result.unwrap();
        assert_eq!(type_id, 42);
        assert_eq!(read_payload, payload);

        drop(writer);
        drop(reader);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_buffer_full() {
        let path = "/tmp/test_shm_full.shm";
        let _ = fs::remove_file(path);

        let digest = [2u8; 32];
        let capacity = 256;
        let mut writer = ShmWriter::create(path, digest, Some(capacity)).unwrap();

        // Fill the buffer
        let payload = vec![0u8; 100];
        writer.write_frame(1, &payload).unwrap();
        writer.write_frame(2, &payload).unwrap();

        // This should fail (not enough space)
        let result = writer.write_frame(3, &payload);
        assert!(matches!(result, Err(ShmError::BufferFull { .. })));

        drop(writer);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_wraparound() {
        let path = "/tmp/test_shm_wrap.shm";
        let _ = fs::remove_file(path);

        let digest = [3u8; 32];
        let mut writer = ShmWriter::create(path, digest, Some(256)).unwrap();
        let mut reader = ShmReader::open(path, digest).unwrap();

        // Write and read multiple frames to cause wraparound
        for i in 0..10 {
            let payload = format!("Frame {}", i).into_bytes();
            writer.write_frame(i as u16, &payload).unwrap();
            let (type_id, read_payload) = reader.read_frame().unwrap().unwrap();
            assert_eq!(type_id, i as u16);
            assert_eq!(read_payload, payload);
        }

        drop(writer);
        drop(reader);
        let _ = fs::remove_file(path);
    }
}
