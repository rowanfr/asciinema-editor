use crate::asciicast_egui::*;
use eframe::egui::Color32;
use memmap::Mmap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

// Size for block processing - can be tuned
const BLOCK_SIZE: usize = 64 * 1024; // 64KB blocks

#[derive(Debug, Clone)]
pub enum ModificationAction {
    Addition(EventPosition),
    Deletion,
    Modification(EventPosition),
}

#[derive(Debug, Clone)]
pub struct EventPosition {
    pub event: Event,
    pub byte_location: usize,
}

/// `CastFile` serves as both a reader and writer to the `.cast` file. The way it works is that it takes in a float between 0 and 1 and maps that to bytes between 0 and the file size. It then reads from that byte selected until it reaches the first newline and then it displays or reads the number of lines requested after that. This editor presumes you're using V2 of the `.cast` file type and thus it expects a JSON header followed by an arbitrary number of newline delimited lines in the format [time, code, data] as shown in the [documentation](https://docs.asciinema.org/manual/asciicast/v2/).
pub struct CastFile {
    /// Owned path to `.cast` file
    pub file_path: PathBuf,
    /// Memory map of the `.cast` file
    mmap: Mmap,
    pub header: Header,
    /// File size for fast computation of location for mmap
    file_size: u64,
    // Map of byte_location -> modification action
    modifications: BTreeMap<usize, ModificationAction>,
}

impl CastFile {
    pub fn new(path: PathBuf) -> Result<Self, CastError> {
        let file = File::open(&path).expect("Failed to Open File");
        let file_size = file.metadata().expect("Failed to Get File Metadata").len();
        // Create read-only memory map so that we can mitigate loading times
        let mmap = unsafe { Mmap::map(&file).expect("Failed to Create Memory Map") };

        // From the beginning of the file go to the first newline to parse header
        let header_end = mmap
            .iter()
            .position(|&b| b == b'\n')
            .expect("Invalid file format");
        let header: Header = serde_json::from_slice(&mmap[..header_end])
            .map_err(|e| CastError::DeserializationError(e.to_string()))?;
        if header.version != 2 {
            return Err(CastError::InvalidVersion);
        }
        Ok(Self {
            file_path: path,
            mmap,
            header,
            file_size,
            modifications: BTreeMap::new(),
        })
    }

    pub fn add_modification(&mut self, byte_location: usize, action: ModificationAction) {
        self.modifications.insert(byte_location, action);
    }

    /// Gets `n` lines starting after the first encountered newline from `pos` (0.0 to 1.0) mapped to bytes of the file from 0 bytes to the end of the file. As it starts after the first newline the header is automatically excluded
    /// Returns a Vec of Events, where each event is [timestamp, event_code, data]
    pub fn get_lines(&self, pos: f32, n: usize) -> Result<Vec<EventPosition>, CastError> {
        // Clamp pos between 0 and 1
        let pos = pos.clamp(0.0, 1.0);

        // Calculate byte position. This is the (0.0 to 1.0) -> (0 to file size) map we were discussing earlier
        let byte_pos = (pos * self.file_size as f32) as usize;

        // Find the next instance of a newline starting from the mapped byte position
        let mut current_pos = {
            // Branching result of a forward search for a newline. We add 1 to both branches as we want the character after the newline
            if let Some(next_newline) = self.mmap[byte_pos..].iter().position(|&b| b == b'\n') {
                byte_pos + next_newline + 1
            } else {
                // If no newline found ahead, try to find the last newline before this position
                self.mmap[..byte_pos]
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map(|p| p + 1)
                    .ok_or_else(|| {
                        CastError::DeserializationError("No Newlines Found in File".to_string())
                    })?
            }
        };

        // todo: Have it to where the number of lines requested is dynamic according to the screen size. From this instead of just looking forward for new line locations we can look in both directions until we reach either the bidirectional sum necessary or both the file end and beginning
        // Find the end position (up to n lines later or end of file)
        let mut end_pos = current_pos;
        let mut newlines_found = 0;

        for (i, &byte) in self.mmap[current_pos..].iter().enumerate() {
            if byte == b'\n' {
                newlines_found += 1;
                if newlines_found == n {
                    end_pos = current_pos + i + 1;
                    break;
                }
            }
        }
        if newlines_found < n {
            end_pos = self.mmap.len();
        }

        // Process all the records at once
        parse_events(&self.mmap[current_pos..end_pos], current_pos)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), CastError> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        self.write_modified_file(writer)
    }

    fn serialize_event(event: &Event) -> Result<Vec<u8>, CastError> {
        serde_json::to_string(event)
            .map(|s| format!("{}\n", s).into_bytes())
            .map_err(|e| CastError::SerializationError(e.to_string()))
    }

    fn write_modified_file(&self, mut writer: impl Write) -> Result<(), CastError> {
        // Write header first
        serde_json::to_writer(&mut writer, &self.header)
            .map_err(|e| CastError::SerializationError(e.to_string()))?;
        writeln!(&mut writer).map_err(|e| CastError::SerializationError(e.to_string()))?;

        let mut current_pos = 0;
        // Find first newline to skip header in mmap
        while current_pos < self.mmap.len() && self.mmap[current_pos] != b'\n' {
            current_pos += 1;
        }
        current_pos += 1; // Skip the newline itself

        let mut mod_iter = self.modifications.iter().peekable();

        while current_pos < self.mmap.len() {
            // Check if there's a modification at the current position
            match mod_iter.peek() {
                Some((&mod_pos, action)) if mod_pos == current_pos => {
                    match action {
                        ModificationAction::Addition(event) => {
                            // Write new event before current line
                            let serialized = Self::serialize_event(&event.event)?;
                            writer.write_all(&serialized)?;

                            // Write original line
                            let line_end = find_next_newline(&self.mmap, current_pos);
                            writer.write_all(&self.mmap[current_pos..line_end])?;
                            current_pos = line_end;
                        }
                        ModificationAction::Deletion => {
                            // Skip to next line
                            current_pos = find_next_newline(&self.mmap, current_pos);
                        }
                        ModificationAction::Modification(new_event) => {
                            // Replace current line with new event
                            let serialized = Self::serialize_event(&new_event.event)?;
                            writer.write_all(&serialized)?;
                            current_pos = find_next_newline(&self.mmap, current_pos);
                        }
                    }
                    mod_iter.next(); // Move to next modification
                }
                Some((&mod_pos, _)) => {
                    // Write until next modification
                    let write_end = std::cmp::min(mod_pos, self.mmap.len());
                    writer.write_all(&self.mmap[current_pos..write_end])?;
                    current_pos = write_end;
                }
                None => {
                    // No more modifications, write rest of file
                    writer.write_all(&self.mmap[current_pos..])?;
                    break;
                }
            }
        }

        writer.flush()?;
        Ok(())
    }
}

// todo make it to where instead of returning none it returns a deserialization error
/// Parse multiple events at once from a byte slice with it's relative start position from the beginning of the file
fn parse_events(slice: &[u8], base_position: usize) -> Result<Vec<EventPosition>, CastError> {
    let input = std::str::from_utf8(slice)?;

    let mut current_position = base_position;
    let mut events = Vec::new();

    for line in input.lines() {
        let line_start = current_position;
        current_position += line.len() + 1; // +1 for newline

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Use the existing Serde deserialization
        match serde_json::from_str::<Event>(line) {
            Ok(event) => {
                events.push(EventPosition {
                    event,
                    byte_location: line_start,
                });
            }
            Err(e) => {
                eprintln!("Failed to parse event at position {}: {}", line_start, e);
                // Optionally: return Err(SerializationError::Json(e))
                // But skipping bad lines might be more robust
                continue;
            }
        }
    }

    Ok(events)
}

#[derive(Error, Debug)]
pub enum CastError {
    #[error("Invalid hex color format: {0}")]
    InvalidHexFormat(String),

    #[error("Invalid {component} component: {error}")]
    InvalidColorComponent {
        component: &'static str,
        error: String,
    },

    #[error("Invalid palette format: {0}")]
    InvalidPaletteFormat(String),

    #[error("Invalid event format: {0}")]
    InvalidEventFormat(String),

    #[error("Invalid version. This only supports the v2 format version for `.cast` files")]
    InvalidVersion,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("UTF-8 conversion error: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    #[error("File system error: {0}")]
    FileSystemError(String),

    #[error("Memory mapping error: {0}")]
    MmapError(String),
}

// Helper function to find next newline position without overwhelming memory usage
fn find_next_newline(buffer: &[u8], start: usize) -> usize {
    buffer[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(buffer.len(), |pos| start + pos + 1)
}
