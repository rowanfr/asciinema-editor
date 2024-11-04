use eframe::egui::Color32;
use memmap::Mmap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::HashMap,
    fmt::{self},
    fs::File,
    path::PathBuf,
};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Header {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    // `default`: If the field is missing in the JSON, it will use the Default implementation (None) instead of throwing an error.
    // `skip_serializing_if = "Option::is_none"`: If the field is None, it will be omitted from the output JSON
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_time_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub fg: Color32,
    pub bg: Color32,
    pub palette: Vec<Color32>,
}

impl Theme {
    // Helper to convert hex string to Color32
    fn color_from_hex(hex: &str) -> Result<Color32, CastError> {
        // Validate basic CSS color hex format
        if !hex.starts_with('#') {
            return Err(CastError::InvalidHexFormat(
                "Color must start with '#'".to_string(),
            ));
        }

        if hex.len() != 7 {
            return Err(CastError::InvalidHexFormat(format!(
                "Expected 7 characters (including #), got {}",
                hex.len()
            )));
        }

        // Validate color hex characters that represent colors
        if !hex[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CastError::InvalidHexFormat(
                "Invalid hex characters found".to_string(),
            ));
        }

        // Parse color components from string slices
        let r =
            u8::from_str_radix(&hex[1..3], 16).map_err(|e| CastError::InvalidColorComponent {
                component: "red",
                error: e.to_string(),
            })?;

        let g =
            u8::from_str_radix(&hex[3..5], 16).map_err(|e| CastError::InvalidColorComponent {
                component: "green",
                error: e.to_string(),
            })?;

        let b =
            u8::from_str_radix(&hex[5..7], 16).map_err(|e| CastError::InvalidColorComponent {
                component: "blue",
                error: e.to_string(),
            })?;

        Ok(Color32::from_rgb(r, g, b))
    }

    // Convert Color32 to css style hex string
    fn color_to_hex(color: Color32) -> String {
        format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
    }

    // This validates the palette to ensure both that it contains colors and that it has either 8 or 16 colors
    fn validate_palette(palette: &[Color32]) -> Result<(), CastError> {
        if palette.is_empty() {
            return Err(CastError::InvalidPaletteFormat(
                "Palette cannot be empty".to_string(),
            ));
        }

        match palette.len() {
            8 | 16 => Ok(()),
            len => Err(CastError::InvalidPaletteFormat(format!(
                "Palette must contain exactly 8 or 16 colors, got {}",
                len
            ))),
        }
    }
}

impl Serialize for Theme {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;

        Theme::validate_palette(&self.palette)
            .map_err(|e| serde::ser::Error::custom(format!("Invalid palette: {}", e)))?;

        let mut state = serializer.serialize_struct("theme", 3)?;

        state.serialize_field("fg", &Theme::color_to_hex(self.fg))?;
        state.serialize_field("bg", &Theme::color_to_hex(self.bg))?;

        // Convert palette Vec<Color32> to colon-separated string
        let palette_string = self
            .palette
            .iter()
            .map(|&color| Theme::color_to_hex(color))
            .collect::<Vec<String>>()
            .join(":");

        state.serialize_field("palette", &palette_string)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ThemeHelper {
            fg: String,
            bg: String,
            palette: String,
        }

        let helper = ThemeHelper::deserialize(deserializer)?;

        let fg = Theme::color_from_hex(&helper.fg).map_err(|e| {
            serde::de::Error::custom(format!("Failed to parse foreground color: {}", e))
        })?;

        let bg = Theme::color_from_hex(&helper.bg).map_err(|e| {
            serde::de::Error::custom(format!("Failed to parse background color: {}", e))
        })?;

        let palette = helper
            .palette
            .split(':')
            .map(Theme::color_from_hex)
            .collect::<Result<Vec<Color32>, CastError>>()
            .map_err(|e| serde::de::Error::custom(format!("Failed to parse palette: {}", e)))?;

        Theme::validate_palette(&palette)
            .map_err(|e| serde::de::Error::custom(format!("Invalid palette: {}", e)))?;

        Ok(Theme { fg, bg, palette })
    }
}

#[derive(Debug)]
pub enum CastError {
    InvalidHexFormat(String),
    InvalidColorComponent {
        component: &'static str,
        error: String,
    },
    InvalidPaletteFormat(String),
    InvalidEventFormat(String),
    SerializationError(String),
    DeserializationError(String),
}

impl fmt::Display for CastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CastError::InvalidHexFormat(msg) => write!(f, "Invalid hex color format: {}", msg),
            CastError::InvalidColorComponent { component, error } => {
                write!(f, "Invalid {} component: {}", component, error)
            }
            CastError::InvalidPaletteFormat(msg) => write!(f, "Invalid palette format: {}", msg),
            CastError::InvalidEventFormat(msg) => write!(f, "Invalid event format: {}", msg),
            CastError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            CastError::DeserializationError(msg) => write!(f, "Deserialization error: {}", msg),
        }
    }
}

impl std::error::Error for CastError {}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Event(pub f64, pub EventCode, pub String);

#[derive(Debug, Clone)]
pub enum EventCode {
    Output,
    Input,
    Marker,
    Resize,
}

impl EventCode {
    fn from_str(s: &str) -> Result<Self, CastError> {
        match s {
            "o" => Ok(EventCode::Output),
            "i" => Ok(EventCode::Input),
            "m" => Ok(EventCode::Marker),
            "r" => Ok(EventCode::Resize),
            _ => Err(CastError::InvalidEventFormat(format!(
                "Invalid event code. Expected 'o', 'i', 'm', or 'r'. Got: {}",
                s
            ))),
        }
    }
}

impl Serialize for EventCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let code = match self {
            EventCode::Output => "o",
            EventCode::Input => "i",
            EventCode::Marker => "m",
            EventCode::Resize => "r",
        };
        serializer.serialize_str(code)
    }
}

impl<'de> Deserialize<'de> for EventCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        EventCode::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// `CastEditor` serves as both a maintained path to the file and as the dynamic memory map for egui. The way it works is that it takes in a float between 0 and 1 and maps that to bytes between 0 and the file size. It then reads from that byte selected until it reaches the first newline and then it displays or reads the number of lines requested after that. This editor presumes you're using V2 of the `.cast` file type and thus it expects a JSON header followed by an arbitrary number of newline delimited lines in the format [time, code, data] as shown in the [documentation](https://docs.asciinema.org/manual/asciicast/v2/).
pub struct CastEditor {
    /// Owned path to `.cast` file
    pub file_path: PathBuf,
    /// Owned path to save file
    pub save_path: Option<PathBuf>,
    /// Header info
    pub header: Header,
    /// Memory map of the `.cast` file
    mmap: Mmap,
    /// File size for fast computation of location for mmap
    file_size: u64,
    /// Modification check
    pub modified: bool,
}

impl CastEditor {
    pub fn new(path: PathBuf) -> Self {
        let file = File::open(&path).expect("Failed to Open File");
        let file_size = file.metadata().expect("Failed to Get File Metadata").len();
        // Create read-only memory map so that we can mitigate loading times
        let mmap = unsafe { Mmap::map(&file).expect("Failed to Create Memory Map") };

        // From the beginning of the file go to the first newline to parse header
        let header_end = mmap
            .iter()
            .position(|&b| b == b'\n')
            .expect("Invalid file format");
        let header: Header =
            serde_json::from_slice(&mmap[..header_end]).expect("Failed to Parse Header");
        // ! This is bad design and should be fixed later
        if header.version != 2 {
            panic!("Only version 2 is supported")
        }
        Self {
            file_path: path,
            save_path: None,
            header,
            mmap,
            file_size,
            modified: false,
        }
    }

    /// Gets `n` lines starting after the first encountered newline from `pos` (0.0 to 1.0) mapped to bytes of the file from 0 bytes to the end of the file. As it starts after the first newline the header is automatically excluded
    /// Returns a Vec of Events, where each event is [timestamp, event_code, data]
    pub fn get_lines(&self, pos: f32, n: usize) -> Vec<Event> {
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
                    // ! It's a bad idea to return the byte position so change this later
                    .unwrap_or(byte_pos)
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
        parse_events(&self.mmap[current_pos..end_pos])
    }
}

/// Parse multiple events at once from a byte slice
fn parse_events(slice: &[u8]) -> Vec<Event> {
    let input = std::str::from_utf8(slice).unwrap_or_default();

    input
        .lines()
        .filter_map(|line| {
            // Remove whitespace and skip empty lines
            let line = line.trim();
            if line.is_empty() {
                return None;
            }

            // Verify the line has the expected [data] format
            if !line.starts_with('[') || !line.ends_with(']') {
                return None;
            }
            // Remove outer brackets
            let line = &line[1..line.len() - 1];

            // Split on first and second comma that aren't inside quotes
            // Initialize state for parsing
            let mut parts = Vec::new(); // Stores a vector of which contains a string for each part of an event
            let mut sanitized_part = String::new(); // Builds up the current part in a sanitized manner with the state machine. It is the intermediately used store for an eventually complete value in `parts`
            let mut in_quotes = false; // Tracks if we're inside quotes
            let mut escaped = false; // Tracks if next char is escaped

            // Process each character with a state machine. The sole purpose of this state machine is to build a resilient way that escaped quotes and commas within quotes don't cause a breakdown in parsing or throw an error. There is no special parsing of any values done within the quotes, for example \n won't bre rendered as a newline and instead will be directly rendered as \n
            /*
            State machine allows for handling
                Escaped quotes (\")
                ANSI escape sequences (\u001b[)
                Embedded newlines (\r\n)
                Commas within the quoted string
            */
            for c in line.chars() {
                match (c, in_quotes, escaped) {
                    // Handle escaped quote and clears escape flag
                    ('"', _, true) => {
                        sanitized_part.push(c);
                        escaped = false;
                    }
                    // Handle escape character next. Set escaped flag and add it
                    ('\\', _, false) => {
                        sanitized_part.push(c);
                        escaped = true;
                    }
                    // Handle unescaped quote. Turn on or off quote state
                    ('"', _, false) => {
                        in_quotes = !in_quotes;
                        sanitized_part.push(c);
                    }
                    // Handle comma outside quotes, valid field separator
                    (',', false, false) => {
                        if !sanitized_part.is_empty() {
                            parts.push(sanitized_part.trim().to_string());
                            sanitized_part.clear();
                        }
                    }
                    // Handle all other characters, add them and clear any escaped flag (remember we're not rendering them at this point so direct copying is desired)
                    (_, _, _) => {
                        sanitized_part.push(c);
                        escaped = false;
                    }
                }
            }

            // Add the remaining characters from the intermediate sanitized part to parts
            if !sanitized_part.is_empty() {
                parts.push(sanitized_part.trim().to_string());
            }

            if parts.len() != 3 {
                eprintln!("Invalid parts length: {} for line: {}", parts.len(), line);
                return None;
            }

            let timestamp = match parts[0].parse::<f64>() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to parse timestamp: {}", e);
                    return None;
                }
            };

            // Parse the event code from second field by removing surrounding quotes. Fixed character size so `1..2` is valid
            let event_code = match EventCode::from_str(&parts[1][1..2]) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("Failed to parse event code: {}", e);
                    return None;
                }
            };

            // Parse the data from the third field by removing surrounding quotes
            let data = parts[2][1..parts[2].len() - 1].to_string();

            Some(Event(timestamp, event_code, data))
        })
        .collect()
}
