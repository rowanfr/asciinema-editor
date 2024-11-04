use std::collections::BTreeMap;
use std::io::{self, BufReader, BufWriter, Read, Write};

use crate::cast_read::{CastError, Event, Header};

#[derive(Debug, Clone)]
pub enum ModificationAction {
    Addition(Event),
    Deletion,
    Modification(Event),
}

#[derive(Debug)]
pub struct CastWriter {
    // Use BTreeMap to maintain modifications in order
    modifications: BTreeMap<usize, ModificationAction>,
    // Track the header if modified
    pub header: Header,
    header_modified: bool,
}

impl CastWriter {
    pub fn new(header: Header) -> Self {
        Self {
            modifications: BTreeMap::new(),
            header,
            header_modified: false,
        }
    }

    pub fn write_modified_file<R: Read, W: Write>(
        &self,
        input: R,
        output: W,
    ) -> Result<(), CastError> {
        let mut reader = BufReader::new(input);
        let mut writer = BufWriter::new(output);

        // Write header first
        if self.header_modified {
            serde_json::to_writer(&mut writer, &self.header)
                .map_err(|e| CastError::SerializationError(e.to_string()))?;
            writeln!(&mut writer).map_err(|e| CastError::SerializationError(e.to_string()))?;
        }

        // Process the file character by character, similar to parse_events
        let mut current_line = 0;
        let mut line_buffer = Vec::new();
        let mut in_quotes = false;
        let mut escaped = false;

        let mut byte_buffer = [0u8; 1024];

        while let Ok(n) = reader.read(&mut byte_buffer) {
            if n == 0 {
                break;
            }

            for &byte in &byte_buffer[..n] {
                match (byte as char, in_quotes, escaped) {
                    // Handle escaped characters
                    ('"', _, true) => {
                        line_buffer.push(byte);
                        escaped = false;
                    }
                    ('\\', _, false) => {
                        line_buffer.push(byte);
                        escaped = true;
                    }
                    // Handle quotes
                    ('"', _, false) => {
                        line_buffer.push(byte);
                        in_quotes = !in_quotes;
                    }
                    // Handle newline outside quotes - this is a line boundary
                    ('\n', false, false) => {
                        // Process any modifications for this line
                        if let Some(action) = self.modifications.get(&current_line) {
                            match action {
                                ModificationAction::Addition(event) => {
                                    // Write the new event
                                    serde_json::to_writer(&mut writer, event).map_err(|e| {
                                        CastError::SerializationError(e.to_string())
                                    })?;
                                    writeln!(&mut writer).map_err(|e| {
                                        CastError::SerializationError(e.to_string())
                                    })?;
                                }
                                ModificationAction::Deletion => {
                                    // Skip writing this line
                                    line_buffer.clear();
                                    current_line += 1;
                                    continue;
                                }
                                ModificationAction::Modification(event) => {
                                    // Write the modified event instead of the original
                                    serde_json::to_writer(&mut writer, event).map_err(|e| {
                                        CastError::SerializationError(e.to_string())
                                    })?;
                                    writeln!(&mut writer).map_err(|e| {
                                        CastError::SerializationError(e.to_string())
                                    })?;
                                    line_buffer.clear();
                                    current_line += 1;
                                    continue;
                                }
                            }
                        }

                        // Write the original line if not deleted or modified
                        writer
                            .write_all(&line_buffer)
                            .map_err(|e| CastError::SerializationError(e.to_string()))?;
                        writer
                            .write_all(&[b'\n'])
                            .map_err(|e| CastError::SerializationError(e.to_string()))?;

                        line_buffer.clear();
                        current_line += 1;
                    }
                    // Handle all other characters
                    (_, _, _) => {
                        line_buffer.push(byte);
                        escaped = false;
                    }
                }
            }
        }

        // Handle any remaining data in the line buffer
        if !line_buffer.is_empty() {
            writer
                .write_all(&line_buffer)
                .map_err(|e| CastError::SerializationError(e.to_string()))?;
        }

        writer
            .flush()
            .map_err(|e| CastError::SerializationError(e.to_string()))?;
        Ok(())
    }
}
