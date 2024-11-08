use eframe::egui::Color32;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::{
    collections::HashMap,
    num::{ParseFloatError, ParseIntError},
};
use thiserror::Error;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Header {
    pub version: u8,
    pub width: u16,
    pub height: u16,
    // `default`: If the field is missing in the JSON, it will use the Default implementation (None) instead of throwing an error.
    // `skip_serializing_if = "Option::is_none"`: If the field is None, it will be omitted from the output JSON
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
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
pub struct Event {
    pub time: f64,
    pub data: EventData,
}

#[derive(Debug, Clone)]
pub enum EventData {
    Output(String),
    Input(String),
    Resize(u16, u16),
    Marker(String),
    Other(char, String),
}

impl EventData {
    /// Get the variant name as a string
    pub fn get_type(&self) -> &'static str {
        match self {
            EventData::Output(_) => "Output",
            EventData::Input(_) => "Input",
            EventData::Resize(_, _) => "Resize",
            EventData::Marker(_) => "Marker",
            EventData::Other(_, _) => "Other",
        }
    }

    /// Get the data contents as a String
    pub fn get_data(&self) -> String {
        match self {
            EventData::Output(s) => s.clone(),
            EventData::Input(s) => s.clone(),
            EventData::Resize(w, h) => format!("{}x{}", w, h),
            EventData::Marker(s) => s.clone(),
            EventData::Other(_, s) => s.clone(),
        }
    }

    /// Get the associated color for each type
    pub fn get_color(&self) -> Color32 {
        match self {
            EventData::Output(_) => Color32::GREEN,
            EventData::Input(_) => Color32::YELLOW,
            EventData::Resize(_, _) => Color32::RED,
            EventData::Marker(_) => Color32::BLUE,
            EventData::Other(c, _) => {
                let bits = *c as u8;
                // Extract 2 bits for each channel
                let r = ((bits & 0b11000000) >> 6) << 6; // Bits 7-6 for red
                let g = ((bits & 0b00110000) >> 4) << 6; // Bits 5-4 for green
                let b = ((bits & 0b00001100) >> 2) << 6; // Bits 3-2 for blue
                let a = ((bits & 0b00000011) >> 0) << 6; // Bits 1-0 for alpha

                Color32::from_rgba_unmultiplied(r, g, b, a)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub fg: Color32,
    pub bg: Color32,
    pub palette: Vec<Color32>,
}

impl Theme {
    /// Helper to convert hex string to Color32
    fn color_from_hex(hex: &str) -> Result<Color32, ThemeError> {
        // Validate basic CSS color hex format
        if !hex.starts_with('#') || hex.len() != 7 {
            return Err(ThemeError::HexFormat(hex.to_string()));
        }

        // Validate and parse color hex characters that represent colors
        let r = u8::from_str_radix(&hex[1..3], 16)?;
        let g = u8::from_str_radix(&hex[3..5], 16)?;
        let b = u8::from_str_radix(&hex[5..7], 16)?;

        Ok(Color32::from_rgb(r, g, b))
    }

    // Convert Color32 to css style hex string
    fn color_to_hex(color: Color32) -> String {
        format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
    }

    // This validates the palette to ensure both that it contains colors and that it has either 8 or 16 colors
    fn validate_palette(palette: &[Color32]) -> Result<(), ThemeError> {
        match palette.len() {
            8 | 16 => Ok(()),
            len => Err(ThemeError::PaletteSize(len)),
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
            .map_err(|e| serde::ser::Error::custom(e.to_string()))?;

        let mut state = serializer.serialize_struct("Theme", 3)?;

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

        let fg = Theme::color_from_hex(&helper.fg)
            .map_err(|e| serde::de::Error::custom(format!("Invalid fg color: {}", e)))?;

        let bg = Theme::color_from_hex(&helper.bg)
            .map_err(|e| serde::de::Error::custom(format!("Invalid bg color: {}", e)))?;

        let palette = helper
            .palette
            .split(':')
            .map(Theme::color_from_hex)
            .collect::<Result<Vec<Color32>, ThemeError>>()
            .map_err(|e| serde::de::Error::custom(format!("Invalid palette color: {}", e)))?;

        Theme::validate_palette(&palette).map_err(|e| serde::de::Error::custom(e.to_string()))?;

        Ok(Theme { fg, bg, palette })
    }
}

impl Serialize for Event {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use EventData::*;

        let (code, data) = match &self.data {
            Output(data) => ('o', data),
            Input(data) => ('i', data),
            Resize(cols, rows) => ('r', &format!("{}x{}", cols, rows)),
            Marker(data) => ('m', data),
            Other(code, data) => (*code, data),
        };

        // Create the formatted string matching the asciinema format
        let formatted = format!(
            "[{}, \"{}\", \"{}\"]",
            self.time,
            code,
            // ! Check the data in case of improper serialization
            data.replace('\"', "\\\"")
        );

        serializer.serialize_str(&formatted)
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        match value {
            // If it's already a string, use it directly
            Value::String(input) => {
                // Helper function to convert our EventError to serde's Error type
                fn convert_err<E: serde::de::Error>(e: EventError) -> E {
                    E::custom(e.to_string())
                }

                // Verify the line has the expected [data] format
                if !input.starts_with('[') || !input.ends_with(']') {
                    return Err(convert_err(EventError::Format(
                        "Event must be wrapped in brackets".to_string(),
                    )));
                }

                // Remove outer brackets
                let input = &input[1..input.len() - 1];

                // Split on first and second comma that aren't inside quotes
                // Initialize state for parsing
                let mut parts = Vec::new(); // Stores a vector of which contains a string for each part of an event
                let mut current = String::new(); // Builds up the current part in a sanitized manner with the state machine. It is the intermediately used store for an eventually complete value in `parts`
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
                for c in input.chars() {
                    match (c, in_quotes, escaped) {
                        // Handle escaped quote and clears escape flag
                        ('"', _, true) => {
                            current.push(c);
                            escaped = false;
                        }
                        // Handle escape character next. Set escaped flag and add it
                        ('\\', _, false) => {
                            escaped = true;
                        }
                        // Handle unescaped quote. Turn on or off quote state
                        ('"', _, false) => {
                            in_quotes = !in_quotes;
                        }
                        // Handle comma outside quotes, valid field separator
                        (',', false, false) => {
                            if !current.is_empty() {
                                parts.push(current.trim().to_string());
                                current.clear();
                            }
                        }
                        // Handle all other characters, add them and clear any escaped flag (remember we're not rendering them at this point so direct copying is desired)
                        (_, _, _) => {
                            current.push(c);
                            escaped = false;
                        }
                    }
                }

                // Add the remaining characters from the intermediate sanitized part to parts
                if !current.is_empty() {
                    parts.push(current.trim().to_string());
                }

                if parts.len() != 3 {
                    return Err(convert_err(EventError::PartCount(parts.len())));
                }

                let time = parts[0]
                    .parse::<f64>()
                    .map_err(EventError::Time)
                    .map_err(convert_err)?;

                let code = parts[1]
                    .trim_matches('"')
                    .chars()
                    .next()
                    .ok_or_else(|| convert_err(EventError::MissingCode))?;

                let data = parts[2].trim_matches('"').to_string();

                let event_data = match code {
                    'o' => EventData::Output(data),
                    'i' => EventData::Input(data),
                    'r' => {
                        let (cols, rows) = data
                            .split_once('x')
                            .ok_or_else(|| EventError::Resize(data.clone()))
                            .map_err(convert_err)?;

                        let cols = cols
                            .parse()
                            .map_err(|_| EventError::Resize(data.clone()))
                            .map_err(convert_err)?;

                        let rows = rows
                            .parse()
                            .map_err(|_| EventError::Resize(data.clone()))
                            .map_err(convert_err)?;

                        EventData::Resize(cols, rows)
                    }
                    'm' => EventData::Marker(data),
                    c => EventData::Other(c, data),
                };

                Ok(Event {
                    time,
                    data: event_data,
                })
            }
            // Handle raw JSON array format - direct parsing
            Value::Array(arr) if arr.len() == 3 => {
                let time = arr[0]
                    .as_f64()
                    .ok_or_else(|| serde::de::Error::custom("First element must be a number"))?;

                let code = arr[1]
                    .as_str()
                    .and_then(|s| s.chars().next())
                    .ok_or_else(|| {
                        serde::de::Error::custom(
                            "Second element must be a string with at least one character",
                        )
                    })?;

                let data = match &arr[2] {
                    Value::String(s) => {
                        // Convert to a JSON value and back to get the escaped string representation
                        serde_json::to_string(s)
                            .map_err(serde::de::Error::custom)?
                            // Remove the surrounding quotes that to_string adds
                            .trim_matches('"')
                            .to_string()
                    }
                    _ => return Err(serde::de::Error::custom("Third element must be a string")),
                };

                let event_data = match code {
                    'o' => EventData::Output(data),
                    'i' => EventData::Input(data),
                    'r' => {
                        let (cols, rows) = data.split_once('x').ok_or_else(|| {
                            serde::de::Error::custom(format!("Invalid resize format: {}", data))
                        })?;

                        let cols = cols.parse().map_err(|_| {
                            serde::de::Error::custom(format!("Invalid column value: {}", cols))
                        })?;

                        let rows = rows.parse().map_err(|_| {
                            serde::de::Error::custom(format!("Invalid row value: {}", rows))
                        })?;

                        EventData::Resize(cols, rows)
                    }
                    'm' => EventData::Marker(data),
                    c => EventData::Other(c, data),
                };

                Ok(Event {
                    time,
                    data: event_data,
                })
            }

            _ => Err(serde::de::Error::custom(
                "Expected string or array of 3 elements",
            )),
        }
    }
}

#[derive(Error, Debug)]
pub enum ThemeError {
    #[error("Invalid color hex format: {0}")]
    HexFormat(String),

    #[error("Invalid color hex value: {0}")]
    HexValue(#[from] ParseIntError),

    #[error("Invalid palette size: expected 8 or 16 colors, got {0}")]
    PaletteSize(usize),
}

#[derive(Error, Debug)]
pub enum EventError {
    #[error("Invalid event format: {0}")]
    Format(String),

    #[error("Invalid event time: {0}")]
    Time(#[from] ParseFloatError),

    #[error("Invalid resize format: expected WxH, got {0}")]
    Resize(String),

    #[error("Missing event code")]
    MissingCode,

    #[error("Invalid number of event parts: expected 3, got {0}")]
    PartCount(usize),
}

#[derive(Error, Debug)]
pub enum SerializationError {
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Theme error: {0}")]
    Theme(#[from] ThemeError),

    #[error("Event error: {0}")]
    Event(#[from] EventError),
}
