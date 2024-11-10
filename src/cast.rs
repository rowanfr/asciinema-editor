use crate::asciicast_egui::*;
use eframe::egui::Color32;
use memmap::Mmap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{BufWriter, Write},
    mem,
    path::{Path, PathBuf},
};
use thiserror::Error;

// Size for block processing - can be tuned
const BLOCK_SIZE: usize = 64 * 1024; // 64KB blocks

#[derive(Debug, Clone)]
pub enum ModificationAction {
    /// Addition prepends is meant to indicate prepending an Event
    Addition(Event),
    /// Delete either removes an addition action or changes the `original_deleted` state
    Deletion,
    /// Only modify the data, not the time
    ModifyData(EventData),
}

/// This represents advanced modification actions which can be thought of as collections of basic modification actions
#[derive(Debug)]
pub enum AdvancedModificationAction {
    /// Modify the current event. Can be thought of as a deletion followed by an addition. This also includes time checking through Addition which ModifyData does not
    Modify(Event),
    /// Swaps the position of 2 events with associated order. Can be thought of as 2 deletions followed by 2 additions. This assumes you're using the function `add_advanced_action` to give context as to the current action, and then passing in the location of the other action into this enum.
    Swap(EventPositioned, usize),
}

/// `ModificationChain` is used to organize modifications at a given byte location. It works by holding a value to check whether or not to render the original and a vector of Events which are the modifications. These modifications are prepended to the memory mapped event they normally point to in implementation.
pub struct ModificationChain {
    pub modifications: Vec<Event>,
    original_deleted: bool,
}

impl ModificationChain {
    fn new() -> Self {
        Self {
            modifications: Vec::new(),
            original_deleted: false,
        }
    }
}

/// A given event with an associated position for rendering and modification
#[derive(Debug, Clone)]
pub struct EventPositioned {
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
    modifications: BTreeMap<usize, ModificationChain>,
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

    // todo enable adding chains instead of just individual actions
    // todo have the result of this be used to inform the completion of actions and thus be used to inform action history of users for undo and redo
    /// Addition action inserts an action into the order specified. Delete action removes any action it points to based on order. If the delete is outside the order available it swaps the original line from on to off.
    pub fn action(
        &mut self,
        action: ModificationAction,
        order: usize,
        current_event: &EventPositioned,
        // This is only needed for timing boundaries in the Addition action
        previous_event: Option<&EventPositioned>,
    ) -> Result<(), CastError> {
        // Get or create the value at the current byte location
        let entry = self
            .modifications
            .entry(current_event.byte_location)
            .or_insert_with(ModificationChain::new);

        let order = order.clamp(0, entry.modifications.len());
        match action {
            // As addition/insertion is between the current and previous event we can check them for time validity
            ModificationAction::Addition(event) => {
                if let Some(previous_event) = previous_event {
                    if previous_event.event.time < event.time
                        && event.time < current_event.event.time
                    {
                        entry.modifications.insert(order, event);
                    } else {
                        return Err(CastError::TimingError);
                    }
                } else {
                    return Err(CastError::UnverifiableTime);
                }
            }
            ModificationAction::Deletion => match entry.modifications.get_mut(order) {
                Some(_) => {
                    entry.modifications.remove(order);
                }
                None => {
                    // If order of delete falls out of range it flips deleting the original
                    entry.original_deleted = !entry.original_deleted;
                }
            },
            ModificationAction::ModifyData(event_data) => {
                match entry.modifications.get_mut(order) {
                    Some(event) => event.data = event_data,
                    None => return Err(CastError::ModificationError),
                }
            }
        };
        Ok(())
    }

    // todo enable adding chains instead of just individual actions
    // todo have the result of this be used to inform the completion of actions and thus be used to inform action history of users for undo and redo
    /// Addition action inserts an action into the order specified. Delete action removes any action it points to based on order. If the delete is outside the order available it swaps the original line from on to off.
    pub fn advanced_action(
        &mut self,
        action: AdvancedModificationAction,
        order: usize,
        current_event: &EventPositioned,
        previous_event: Option<&EventPositioned>,
        // todo change window to 3. Handle first by passing in 0 for first timing or f64 max for end timing. Change event position references to time values instead as that's all we're grabbing
        next_event: Option<&EventPositioned>,
    ) -> Result<(), CastError> {
        // Get or create th value at the current byte location
        let entry = self
            .modifications
            .entry(current_event.byte_location)
            .or_insert_with(ModificationChain::new);

        let order = order.clamp(0, entry.modifications.len());
        match action {
            AdvancedModificationAction::Modify(event) => {
                if let Some(next_event) = next_event {
                    if let Some(previous_event) = previous_event {
                        // First action's is deleting what you're pointing to
                        self.action(ModificationAction::Deletion, order, current_event, None)?;
                        // Then we add an action that is the edited event into the topmost region of the next event. We know the topmost region will be order 0 as it addition prepends events sequentially in vector order, thus 0 is first
                        self.action(
                            ModificationAction::Addition(event),
                            0,
                            next_event,
                            Some(previous_event),
                        )?;
                    } else {
                        return Err(CastError::UnverifiableTime);
                    }
                } else {
                    return Err(CastError::UnverifiableTime);
                }
            }
            AdvancedModificationAction::Swap(target_event, target_order) => {
                let current_data = current_event.event.data.clone();
                let targeted_data = target_event.event.data.clone();
                self.action(
                    ModificationAction::ModifyData(targeted_data),
                    order,
                    current_event,
                    None,
                );
                self.action(
                    ModificationAction::ModifyData(current_data),
                    target_order,
                    &target_event,
                    None,
                );
            }
        };
        Ok(())
    }

    // todo: We currently just assume that there will always be a requested time value due to the only modification of time being through the advanced modify action but we should likely have additional checks
    /// This works on getting the order of the base events. It also operates under the presumption that *there are no duplicate time values for any event and that time events are ordered*. It returns either the order location or None if it is not present or there is no modification chain associated with that byte.

    pub fn get_order(&self, byte_location: usize, base_event: &Event) -> usize {
        self.modifications
            .get(&byte_location)
            // If no chain is found then return 0 as action can handle both if the chain exists or doesn't with the expected order of 0 for most behavior. When the first action order 0 works both for insertions and swapping the reading of the mmap event
            .map_or(0, |chain| {
                chain
                    .modifications
                    .iter()
                    .position(|event| event.time == base_event.time)
                    // ! If number isn't there then it should result in the maximum len of the chain. This allows delete on the original mmap event to work using similar syntax to other actions as the delete will target the len which necessarily flips if the original is being rendered.
                    .unwrap_or(chain.modifications.len())
            })
    }

    /// Gets `n` lines starting after the first encountered newline from `pos` (0.0 to 1.0) mapped to bytes of the file from 0 bytes to the end of the file. As it starts after the first newline the header is automatically excluded
    /// Returns a Vec of Events, where each event is [timestamp, event_code, data]
    pub fn get_lines(&self, pos: f32, n: usize) -> Result<Vec<EventPositioned>, CastError> {
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
        // If fewer newlines found than requested return all we have until the end of the file
        if newlines_found < n {
            end_pos = self.mmap.len();
        }

        // Process the range and apply modifications
        let mut events = Vec::new();
        let mut mod_iter = self.modifications.range(current_pos..end_pos).peekable();

        while current_pos < end_pos {
            match mod_iter.peek() {
                Some((&mod_pos, chain)) if mod_pos == current_pos => {
                    // byte location for all actions before any potential delete action alters it
                    let byte_location = current_pos;
                    for event in chain.modifications.clone() {
                        // Add the new event before current position so this is being pre-pended
                        events.push(EventPositioned {
                            event,
                            byte_location,
                        });
                    }
                    if chain.original_deleted {
                        // Skip this original line in the mmap
                        current_pos = find_next_newline(&self.mmap, current_pos);
                    }
                    mod_iter.next(); // Move to next modification
                }
                Some((&mod_pos, _)) => {
                    // Parse events until the next modification
                    let parse_end = std::cmp::min(mod_pos, end_pos);
                    if let Ok(mut parsed_events) =
                        parse_events(&self.mmap[current_pos..parse_end], current_pos)
                    {
                        events.extend(parsed_events);
                    }
                    current_pos = parse_end;
                }
                None => {
                    // No more modifications, parse remaining events in range
                    if let Ok(mut parsed_events) =
                        parse_events(&self.mmap[current_pos..end_pos], current_pos)
                    {
                        events.extend(parsed_events);
                    }
                    break;
                }
            }
        }

        Ok(events)
    }

    // !todo make it to where when you save to a file you remove the current cast file in memory and reconstruct a Cast file handle pointing to the new file to free memory used for in-memory action history
    pub fn save_to_file(&self, path: &Path) -> Result<(), CastError> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        self.write_modified_file(writer)
    }

    // ! This removes spaces but it can still be read so I'll deal with that later
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
                Some((&mod_pos, chain)) if mod_pos == current_pos => {
                    for event in chain.modifications.clone() {
                        // Write new event before current line
                        let serialized = Self::serialize_event(&event)?;
                        writer.write_all(&serialized)?;
                    }
                    if chain.original_deleted {
                        // Skip this original line in the mmap
                        current_pos = find_next_newline(&self.mmap, current_pos);
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

/// Parse multiple events at once from a byte slice with it's relative start position from the beginning of the file
fn parse_events(slice: &[u8], base_position: usize) -> Result<Vec<EventPositioned>, CastError> {
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
                events.push(EventPositioned {
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

    #[error("Invalid timing modification request. Event timing must always be between the previous and next event, with a minimum value of 0")]
    TimingError,

    #[error("Invalid modification request. The order is likely out of bounds")]
    ModificationError,

    #[error("No contextualizing event was passed in thus timing boundaries are unverifiable")]
    UnverifiableTime,
}

// Helper function to find next newline position without overwhelming memory usage
fn find_next_newline(buffer: &[u8], start: usize) -> usize {
    buffer[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(buffer.len(), |pos| start + pos + 1)
}
