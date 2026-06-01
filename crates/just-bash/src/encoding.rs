use crate::error::{JustBashError, JustBashErrorKind, JustBashResult};

/// File content passed to write and append operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileContent {
    Text(String),
    Bytes(Vec<u8>),
}

impl From<&str> for FileContent {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for FileContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Vec<u8>> for FileContent {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

impl From<&[u8]> for FileContent {
    fn from(value: &[u8]) -> Self {
        Self::Bytes(value.to_vec())
    }
}

/// Supported text and byte encodings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BufferEncoding {
    #[default]
    Utf8,
    Ascii,
    Binary,
    Base64,
    Hex,
    Latin1,
}

impl BufferEncoding {
    /// Parses the encoding names accepted by upstream Just Bash.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "utf8" | "utf-8" => Some(Self::Utf8),
            "ascii" => Some(Self::Ascii),
            "binary" => Some(Self::Binary),
            "base64" => Some(Self::Base64),
            "hex" => Some(Self::Hex),
            "latin1" => Some(Self::Latin1),
            _ => None,
        }
    }
}

/// Converts file content to bytes using the requested encoding.
pub fn content_to_bytes(
    content: impl Into<FileContent>,
    encoding: BufferEncoding,
) -> JustBashResult<Vec<u8>> {
    match content.into() {
        FileContent::Bytes(bytes) => Ok(bytes),
        FileContent::Text(text) => match encoding {
            BufferEncoding::Utf8 => Ok(text.into_bytes()),
            BufferEncoding::Ascii => Ok(text.bytes().map(|byte| byte & 0x7f).collect()),
            BufferEncoding::Binary | BufferEncoding::Latin1 => latin1_to_bytes(&text),
            BufferEncoding::Base64 => decode_base64(&text),
            BufferEncoding::Hex => decode_hex(&text),
        },
    }
}

/// Converts bytes to a string using the requested encoding.
pub fn bytes_to_string(bytes: &[u8], encoding: BufferEncoding) -> String {
    match encoding {
        BufferEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        BufferEncoding::Ascii => bytes.iter().map(|byte| char::from(byte & 0x7f)).collect(),
        BufferEncoding::Binary | BufferEncoding::Latin1 => bytes_to_latin1(bytes),
        BufferEncoding::Base64 => encode_base64(bytes),
        BufferEncoding::Hex => encode_hex(bytes),
    }
}

fn latin1_to_bytes(text: &str) -> JustBashResult<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.chars().count());
    for character in text.chars() {
        let value = character as u32;
        if value > 0xff {
            return Err(JustBashError::new(
                JustBashErrorKind::InvalidInput,
                "encode",
                "<content>",
                "latin1 character out of range",
            ));
        }
        bytes.push(value as u8);
    }
    Ok(bytes)
}

fn bytes_to_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

fn decode_hex(text: &str) -> JustBashResult<Vec<u8>> {
    if text.len() % 2 != 0 {
        return Err(JustBashError::new(
            JustBashErrorKind::InvalidInput,
            "decode",
            "<hex>",
            "hex input has odd length",
        ));
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let chars: Vec<char> = text.chars().collect();
    for chunk in chars.chunks(2) {
        let high = chunk[0].to_digit(16).ok_or_else(|| {
            JustBashError::new(
                JustBashErrorKind::InvalidInput,
                "decode",
                "<hex>",
                "invalid hex digit",
            )
        })?;
        let low = chunk[1].to_digit(16).ok_or_else(|| {
            JustBashError::new(
                JustBashErrorKind::InvalidInput,
                "decode",
                "<hex>",
                "invalid hex digit",
            )
        })?;
        bytes.push(((high << 4) | low) as u8);
    }
    Ok(bytes)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
}

fn decode_base64(text: &str) -> JustBashResult<Vec<u8>> {
    let compact: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if compact.len() % 4 != 0 {
        return Err(JustBashError::new(
            JustBashErrorKind::InvalidInput,
            "decode",
            "<base64>",
            "invalid base64 length",
        ));
    }

    let mut output = Vec::with_capacity(compact.len() / 4 * 3);
    for quartet in compact.chunks(4) {
        let mut values = [0_u8; 4];
        let mut padding = 0;
        for (index, byte) in quartet.iter().copied().enumerate() {
            if byte == b'=' {
                values[index] = 0;
                padding += 1;
            } else if let Some(value) = base64_value(byte) {
                values[index] = value;
            } else {
                return Err(JustBashError::new(
                    JustBashErrorKind::InvalidInput,
                    "decode",
                    "<base64>",
                    "invalid base64 character",
                ));
            }
        }
        let triple = [
            (values[0] << 2) | (values[1] >> 4),
            (values[1] << 4) | (values[2] >> 2),
            (values[2] << 6) | values[3],
        ];
        output.push(triple[0]);
        if padding < 2 {
            output.push(triple[1]);
        }
        if padding < 1 {
            output.push(triple[2]);
        }
    }
    Ok(output)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        encoded.push(char::from(ALPHABET[(first >> 2) as usize]));
        encoded.push(char::from(
            ALPHABET[(((first & 0x03) << 4) | (second >> 4)) as usize],
        ));
        if chunk.len() > 1 {
            encoded.push(char::from(
                ALPHABET[(((second & 0x0f) << 2) | (third >> 6)) as usize],
            ));
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(char::from(ALPHABET[(third & 0x3f) as usize]));
        } else {
            encoded.push('=');
        }
    }
    encoded
}

/// A byte buffer that mirrors upstream's latin1-shaped `ByteString`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ByteString(Vec<u8>);

impl ByteString {
    /// Creates a byte string from raw bytes.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Creates a byte string from a latin1-shaped string.
    pub fn from_latin1(value: &str) -> JustBashResult<Self> {
        latin1_to_bytes(value).map(Self)
    }

    /// Returns the raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Converts the bytes to a latin1-shaped string.
    pub fn to_latin1(&self) -> String {
        bytes_to_latin1(&self.0)
    }

    /// Decodes UTF-8 when valid, otherwise returns the latin1 view.
    pub fn decode_utf8_or_latin1(&self) -> String {
        String::from_utf8(self.0.clone()).unwrap_or_else(|_| self.to_latin1())
    }
}

impl From<Vec<u8>> for ByteString {
    fn from(value: Vec<u8>) -> Self {
        Self::from_bytes(value)
    }
}

/// Output shape for redirection and pipeline boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputPayload {
    Text(String),
    Bytes(ByteString),
}

impl OutputPayload {
    /// Converts stdout to the bytes the shell pipe carries.
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::Text(text) => text.into_bytes(),
            Self::Bytes(bytes) => bytes.0,
        }
    }
}
