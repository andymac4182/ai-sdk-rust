use crate::error::{Result, WorkflowCoreError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base64Alphabet {
    Base64,
    Base64Url,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastChunkHandling {
    Loose,
    Strict,
    StopBeforePartial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToBase64Options {
    pub alphabet: Base64Alphabet,
    pub omit_padding: bool,
}

impl Default for ToBase64Options {
    fn default() -> Self {
        Self {
            alphabet: Base64Alphabet::Base64,
            omit_padding: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FromBase64Options {
    pub alphabet: Base64Alphabet,
    pub last_chunk_handling: LastChunkHandling,
}

impl Default for FromBase64Options {
    fn default() -> Self {
        Self {
            alphabet: Base64Alphabet::Base64,
            last_chunk_handling: LastChunkHandling::Loose,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetFromResult {
    pub read: usize,
    pub written: usize,
}

const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn lookup_base64(byte: u8) -> Option<u8> {
    BASE64_CHARS
        .iter()
        .position(|value| *value == byte)
        .map(|index| index as u8)
}

fn is_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn skip_ascii_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && is_ascii_whitespace(bytes[index]) {
        index += 1;
    }
    index
}

fn decode_base64_chunk(chunk: &[u8], throw_on_extra_bits: bool) -> Result<Vec<u8>> {
    let chunk_length = chunk.len();
    let mut padded = [b'A'; 4];
    for (index, byte) in chunk.iter().copied().enumerate() {
        padded[index] = byte;
    }

    let b0 = lookup_base64(padded[0]).ok_or_else(|| {
        WorkflowCoreError::InvalidBase64("unexpected character in chunk".to_string())
    })?;
    let b1 = lookup_base64(padded[1]).ok_or_else(|| {
        WorkflowCoreError::InvalidBase64("unexpected character in chunk".to_string())
    })?;
    let b2 = lookup_base64(padded[2]).ok_or_else(|| {
        WorkflowCoreError::InvalidBase64("unexpected character in chunk".to_string())
    })?;
    let b3 = lookup_base64(padded[3]).ok_or_else(|| {
        WorkflowCoreError::InvalidBase64("unexpected character in chunk".to_string())
    })?;

    let byte0 = (b0 << 2) | (b1 >> 4);
    let byte1 = ((b1 & 0x0f) << 4) | (b2 >> 2);
    let byte2 = ((b2 & 0x03) << 6) | b3;

    match chunk_length {
        2 => {
            if throw_on_extra_bits && byte1 != 0 {
                return Err(WorkflowCoreError::InvalidBase64(
                    "Extra bits in base64 chunk".to_string(),
                ));
            }
            Ok(vec![byte0])
        }
        3 => {
            if throw_on_extra_bits && byte2 != 0 {
                return Err(WorkflowCoreError::InvalidBase64(
                    "Extra bits in base64 chunk".to_string(),
                ));
            }
            Ok(vec![byte0, byte1])
        }
        _ => Ok(vec![byte0, byte1, byte2]),
    }
}

#[derive(Debug)]
struct FromBase64Result {
    read: usize,
    bytes: Vec<u8>,
}

fn from_base64_inner(
    input: &str,
    options: FromBase64Options,
    max_length: Option<usize>,
) -> Result<FromBase64Result> {
    let max_length = max_length.unwrap_or(usize::MAX);
    if max_length == 0 {
        return Ok(FromBase64Result {
            read: 0,
            bytes: Vec::new(),
        });
    }

    let bytes = input.as_bytes();
    let mut read = 0;
    let mut out = Vec::new();
    let mut chunk = Vec::new();
    let mut index = 0;

    loop {
        index = skip_ascii_whitespace(bytes, index);

        if index == bytes.len() {
            if !chunk.is_empty() {
                match options.last_chunk_handling {
                    LastChunkHandling::StopBeforePartial => {
                        return Ok(FromBase64Result { read, bytes: out });
                    }
                    LastChunkHandling::Loose => {
                        if chunk.len() == 1 {
                            return Err(WorkflowCoreError::InvalidBase64(
                                "lone character in final chunk".to_string(),
                            ));
                        }
                        out.extend(decode_base64_chunk(&chunk, false)?);
                    }
                    LastChunkHandling::Strict => {
                        return Err(WorkflowCoreError::InvalidBase64(
                            "incomplete chunk in strict mode".to_string(),
                        ));
                    }
                }
            }
            return Ok(FromBase64Result {
                read: bytes.len(),
                bytes: out,
            });
        }

        let original = bytes[index];
        index += 1;

        if original == b'=' {
            if chunk.len() < 2 {
                return Err(WorkflowCoreError::InvalidBase64(
                    "padding in unexpected place".to_string(),
                ));
            }

            index = skip_ascii_whitespace(bytes, index);
            if chunk.len() == 2 {
                if index == bytes.len() {
                    if options.last_chunk_handling == LastChunkHandling::StopBeforePartial {
                        return Ok(FromBase64Result { read, bytes: out });
                    }
                    return Err(WorkflowCoreError::InvalidBase64(
                        "missing second padding character".to_string(),
                    ));
                }
                if bytes[index] == b'=' {
                    index = skip_ascii_whitespace(bytes, index + 1);
                }
            }
            if index < bytes.len() {
                return Err(WorkflowCoreError::InvalidBase64(
                    "unexpected characters after padding".to_string(),
                ));
            }
            out.extend(decode_base64_chunk(
                &chunk,
                options.last_chunk_handling == LastChunkHandling::Strict,
            )?);
            return Ok(FromBase64Result {
                read: bytes.len(),
                bytes: out,
            });
        }

        let normalized = match options.alphabet {
            Base64Alphabet::Base64 => original,
            Base64Alphabet::Base64Url => match original {
                b'+' | b'/' => {
                    return Err(WorkflowCoreError::InvalidBase64(
                        "unexpected base64url character".to_string(),
                    ));
                }
                b'-' => b'+',
                b'_' => b'/',
                other => other,
            },
        };

        if lookup_base64(normalized).is_none() {
            return Err(WorkflowCoreError::InvalidBase64(format!(
                "unexpected character '{}'",
                original as char
            )));
        }

        let remaining = max_length.saturating_sub(out.len());
        if (remaining == 1 && chunk.len() == 2) || (remaining == 2 && chunk.len() == 3) {
            return Ok(FromBase64Result { read, bytes: out });
        }

        chunk.push(normalized);
        if chunk.len() == 4 {
            out.extend(decode_base64_chunk(&chunk, false)?);
            chunk.clear();
            read = index;
            if out.len() == max_length {
                return Ok(FromBase64Result { read, bytes: out });
            }
        }
    }
}

pub fn to_base64(bytes: &[u8], options: ToBase64Options) -> String {
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        result.push(BASE64_CHARS[((b0 >> 2) & 0x3f) as usize] as char);
        result.push(BASE64_CHARS[(((b0 & 0x03) << 4) | ((b1 >> 4) & 0x0f)) as usize] as char);

        if chunk.len() > 1 {
            result.push(BASE64_CHARS[(((b1 & 0x0f) << 2) | ((b2 >> 6) & 0x03)) as usize] as char);
        } else if !options.omit_padding {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(BASE64_CHARS[(b2 & 0x3f) as usize] as char);
        } else if !options.omit_padding {
            result.push('=');
        }
    }

    if options.alphabet == Base64Alphabet::Base64Url {
        result = result.replace('+', "-").replace('/', "_");
    }
    result
}

pub fn from_base64(input: &str, options: FromBase64Options) -> Result<Vec<u8>> {
    from_base64_inner(input, options, None).map(|result| result.bytes)
}

pub fn set_from_base64(
    target: &mut [u8],
    input: &str,
    options: FromBase64Options,
) -> Result<SetFromResult> {
    let result = from_base64_inner(input, options, Some(target.len()))?;
    for (slot, value) in target.iter_mut().zip(result.bytes.iter().copied()) {
        *slot = value;
    }
    Ok(SetFromResult {
        read: result.read,
        written: result.bytes.len(),
    })
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex_inner(input: &str, max_length: Option<usize>) -> Result<FromBase64Result> {
    if input.len() % 2 != 0 {
        return Err(WorkflowCoreError::InvalidHex(
            "string length must be even".to_string(),
        ));
    }
    let max_length = max_length.unwrap_or(usize::MAX);
    let mut out = Vec::new();
    let mut read = 0;
    while read < input.len() && out.len() < max_length {
        let pair = &input[read..read + 2];
        if !pair.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(WorkflowCoreError::InvalidHex(format!(
                "unexpected character at position {read}"
            )));
        }
        out.push(u8::from_str_radix(pair, 16).expect("hex pair was validated"));
        read += 2;
    }
    Ok(FromBase64Result { read, bytes: out })
}

pub fn from_hex(input: &str) -> Result<Vec<u8>> {
    from_hex_inner(input, None).map(|result| result.bytes)
}

pub fn set_from_hex(target: &mut [u8], input: &str) -> Result<SetFromResult> {
    let result = from_hex_inner(input, Some(target.len()))?;
    for (slot, value) in target.iter_mut().zip(result.bytes.iter().copied()) {
        *slot = value;
    }
    Ok(SetFromResult {
        read: result.read,
        written: result.bytes.len(),
    })
}

pub fn btoa(input: &str) -> String {
    to_base64(input.as_bytes(), ToBase64Options::default())
}

pub fn atob(input: &str) -> Result<String> {
    let bytes = from_base64(input, FromBase64Options::default())?;
    String::from_utf8(bytes).map_err(|error| WorkflowCoreError::InvalidBase64(error.to_string()))
}

pub fn create_random_uuid<F>(mut rng: F) -> impl FnMut() -> String
where
    F: FnMut() -> f64,
{
    move || random_uuid_from_rng(&mut rng)
}

pub fn random_uuid_from_rng(rng: &mut impl FnMut() -> f64) -> String {
    let chars = b"0123456789abcdef";
    let mut uuid = String::with_capacity(36);
    for index in 0..36 {
        match index {
            8 | 13 | 18 | 23 => uuid.push('-'),
            14 => uuid.push('4'),
            19 => {
                let value = ((rng() * 4.0).floor() as usize).min(3) + 8;
                uuid.push(chars[value] as char);
            }
            _ => {
                let value = ((rng() * 16.0).floor() as usize).min(15);
                uuid.push(chars[value] as char);
            }
        }
    }
    uuid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_rng(values: Vec<f64>) -> impl FnMut() -> f64 {
        let mut index = 0;
        move || {
            let value = values[index % values.len()];
            index += 1;
            value
        }
    }

    fn is_valid_uuid_v4(uuid: &str) -> bool {
        let bytes = uuid.as_bytes();
        uuid.len() == 36
            && bytes[8] == b'-'
            && bytes[13] == b'-'
            && bytes[18] == b'-'
            && bytes[23] == b'-'
            && bytes[14] == b'4'
            && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
            && uuid
                .chars()
                .enumerate()
                .all(|(index, char)| matches!(index, 8 | 13 | 18 | 23) || char.is_ascii_hexdigit())
    }

    #[test]
    fn vm_uint8array_base64_test_to_base64_rows_match_upstream() {
        assert_eq!(
            to_base64(b"Hello World", ToBase64Options::default()),
            "SGVsbG8gV29ybGQ="
        );
        assert_eq!(to_base64(&[], ToBase64Options::default()), "");
        assert_eq!(to_base64(&[72], ToBase64Options::default()), "SA==");
        assert_eq!(to_base64(&[72, 101], ToBase64Options::default()), "SGU=");
        assert_eq!(
            to_base64(
                &[251, 255, 191],
                ToBase64Options {
                    alphabet: Base64Alphabet::Base64Url,
                    omit_padding: false
                }
            ),
            "-_-_"
        );
        assert_eq!(
            to_base64(
                &[251, 255, 191],
                ToBase64Options {
                    alphabet: Base64Alphabet::Base64,
                    omit_padding: false
                }
            ),
            "+/+/"
        );
        assert_eq!(
            to_base64(
                &[72],
                ToBase64Options {
                    alphabet: Base64Alphabet::Base64,
                    omit_padding: true
                }
            ),
            "SA"
        );
    }

    #[test]
    fn vm_uint8array_base64_test_to_hex_rows_match_upstream() {
        assert_eq!(to_hex(b"Hello World"), "48656c6c6f20576f726c64");
        assert_eq!(to_hex(&[]), "");
        assert_eq!(to_hex(&[0, 1, 15]), "00010f");
        assert_eq!(to_hex(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn vm_uint8array_base64_test_from_base64_rows_match_upstream() {
        assert_eq!(
            from_base64("SGVsbG8gV29ybGQ=", FromBase64Options::default()).unwrap(),
            b"Hello World"
        );
        assert_eq!(from_base64("", FromBase64Options::default()).unwrap(), b"");
        assert_eq!(
            from_base64("SGVs bG8g\nV29y bGQ=", FromBase64Options::default()).unwrap(),
            b"Hello World"
        );
        assert_eq!(
            from_base64("SGVsbG8gV29ybGQ", FromBase64Options::default()).unwrap(),
            b"Hello World"
        );
        assert_eq!(
            from_base64(
                "-_-_",
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64Url,
                    last_chunk_handling: LastChunkHandling::Loose
                }
            )
            .unwrap(),
            vec![251, 255, 191]
        );
        assert!(
            from_base64(
                "+/+/",
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64Url,
                    last_chunk_handling: LastChunkHandling::Loose
                }
            )
            .is_err()
        );
        assert!(from_base64("$$$$", FromBase64Options::default()).is_err());
        assert!(from_base64("A", FromBase64Options::default()).is_err());
    }

    #[test]
    fn vm_uint8array_base64_test_from_base64_strict_and_stop_before_partial_rows_match_upstream() {
        assert!(
            from_base64(
                "SGVsbG8gV29ybGQ",
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64,
                    last_chunk_handling: LastChunkHandling::Strict
                }
            )
            .is_err()
        );
        assert!(
            from_base64(
                "SGVsbG8gV29ybGR=",
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64,
                    last_chunk_handling: LastChunkHandling::Strict
                }
            )
            .is_err()
        );
        assert_eq!(
            from_base64(
                "SGVsbG8gV29ybGQ=",
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64,
                    last_chunk_handling: LastChunkHandling::Strict
                }
            )
            .unwrap(),
            b"Hello World"
        );
        assert_eq!(
            from_base64(
                "SGVsbG8gV29ybGQ",
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64,
                    last_chunk_handling: LastChunkHandling::StopBeforePartial
                }
            )
            .unwrap(),
            b"Hello Wor"
        );
        assert_eq!(
            from_base64(
                "SGVs",
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64,
                    last_chunk_handling: LastChunkHandling::StopBeforePartial
                }
            )
            .unwrap(),
            b"Hel"
        );
    }

    #[test]
    fn vm_uint8array_base64_test_from_hex_rows_match_upstream() {
        assert_eq!(from_hex("48656c6c6f20576f726c64").unwrap(), b"Hello World");
        assert_eq!(from_hex("").unwrap(), b"");
        assert_eq!(from_hex("DEADBEEF").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(from_hex("DeAdBeEf").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert!(from_hex("abc").is_err());
        assert!(from_hex("gg").is_err());
    }

    #[test]
    fn vm_uint8array_base64_test_set_from_base64_rows_match_upstream() {
        let mut target = [0; 7];
        let result =
            set_from_base64(&mut target, "Zm9vYmFy", FromBase64Options::default()).unwrap();
        assert_eq!(
            result,
            SetFromResult {
                read: 8,
                written: 6
            }
        );
        assert_eq!(target, [102, 111, 111, 98, 97, 114, 0]);

        let mut small = [0; 3];
        let result = set_from_base64(&mut small, "Zm9vYmFy", FromBase64Options::default()).unwrap();
        assert_eq!(result.written, 3);
        assert_eq!(small, [102, 111, 111]);

        let mut url = [0; 3];
        let result = set_from_base64(
            &mut url,
            "-_-_",
            FromBase64Options {
                alphabet: Base64Alphabet::Base64Url,
                last_chunk_handling: LastChunkHandling::Loose,
            },
        )
        .unwrap();
        assert_eq!(result.written, 3);
        assert_eq!(url, [251, 255, 191]);
    }

    #[test]
    fn vm_uint8array_base64_test_set_from_hex_rows_match_upstream() {
        let mut target = [0; 6];
        let result = set_from_hex(&mut target, "deadbeef").unwrap();
        assert_eq!(
            result,
            SetFromResult {
                read: 8,
                written: 4
            }
        );
        assert_eq!(target, [0xde, 0xad, 0xbe, 0xef, 0, 0]);

        let mut small = [0; 2];
        let result = set_from_hex(&mut small, "deadbeef").unwrap();
        assert_eq!(result.written, 2);
        assert_eq!(small, [0xde, 0xad]);
        assert!(set_from_hex(&mut [0; 4], "abc").is_err());
    }

    #[test]
    fn vm_uint8array_base64_test_roundtrip_encoding_decoding_rows_match_upstream() {
        let original = [0, 1, 2, 127, 128, 255];
        assert_eq!(
            from_base64(
                &to_base64(&original, ToBase64Options::default()),
                FromBase64Options::default()
            )
            .unwrap(),
            original
        );
        let url_original = [251, 255, 191, 0, 63];
        assert_eq!(
            from_base64(
                &to_base64(
                    &url_original,
                    ToBase64Options {
                        alphabet: Base64Alphabet::Base64Url,
                        omit_padding: false,
                    }
                ),
                FromBase64Options {
                    alphabet: Base64Alphabet::Base64Url,
                    last_chunk_handling: LastChunkHandling::Loose,
                }
            )
            .unwrap(),
            url_original
        );
        assert_eq!(from_hex(&to_hex(&original)).unwrap(), original);
        assert_eq!(
            atob(&btoa("api_key:api_secret")).unwrap(),
            "api_key:api_secret"
        );
    }

    #[test]
    fn vm_uuid_test_basic_functionality_and_v4_spec_rows_match_upstream() {
        let mut random_uuid = create_random_uuid(|| 0.5);
        let uuid = random_uuid();

        assert_eq!(uuid.len(), 36);
        assert!(is_valid_uuid_v4(&uuid));
        assert_eq!(uuid.as_bytes()[8], b'-');
        assert_eq!(uuid.as_bytes()[13], b'-');
        assert_eq!(uuid.as_bytes()[18], b'-');
        assert_eq!(uuid.as_bytes()[23], b'-');
        assert_eq!(uuid.as_bytes()[14], b'4');

        for (value, expected) in [
            (0.0, '8'),
            (0.24, '8'),
            (0.25, '9'),
            (0.49, '9'),
            (0.5, 'a'),
            (0.74, 'a'),
            (0.75, 'b'),
            (0.99, 'b'),
        ] {
            let mut random_uuid = create_random_uuid(move || value);
            assert_eq!(random_uuid().as_bytes()[19] as char, expected);
        }
    }

    #[test]
    fn vm_uuid_test_deterministic_behavior_rows_match_upstream() {
        let mut random_uuid1 = create_random_uuid(mock_rng(vec![0.1, 0.2, 0.3, 0.4, 0.5]));
        let mut random_uuid2 = create_random_uuid(mock_rng(vec![0.1, 0.2, 0.3, 0.4, 0.5]));
        assert_eq!(random_uuid1(), random_uuid2());

        let mut random_uuid1 = create_random_uuid(mock_rng(vec![0.1, 0.2, 0.3]));
        let mut random_uuid2 = create_random_uuid(mock_rng(vec![0.7, 0.8, 0.9]));
        assert_ne!(random_uuid1(), random_uuid2());

        let mut random_uuid = create_random_uuid(mock_rng(vec![0.1, 0.2, 0.3, 0.4, 0.5]));
        let uuid1 = random_uuid();
        let uuid2 = random_uuid();
        assert_ne!(uuid1, uuid2);
        assert!(is_valid_uuid_v4(&uuid1));
        assert!(is_valid_uuid_v4(&uuid2));
    }

    #[test]
    fn vm_uuid_test_edge_cases_and_distribution_rows_match_upstream() {
        let mut zero_uuid = create_random_uuid(|| 0.0);
        assert_eq!(zero_uuid(), "00000000-0000-4000-8000-000000000000");

        let mut near_one_uuid = create_random_uuid(|| 0.999);
        assert_eq!(near_one_uuid(), "ffffffff-ffff-4fff-bfff-ffffffffffff");

        let mut patterned_uuid = create_random_uuid(mock_rng(vec![0.0, 0.5, 0.999]));
        assert_eq!(patterned_uuid(), "08f08f08-f08f-408f-88f0-8f08f08f08f0");

        let values = (0..16).map(|value| value as f64 / 16.0).collect();
        let mut random_uuid = create_random_uuid(mock_rng(values));
        assert!(is_valid_uuid_v4(&random_uuid()));

        for value in [0.0, 0.25, 0.5, 0.75, 0.99] {
            let mut random_uuid = create_random_uuid(move || value);
            assert!(matches!(
                random_uuid().as_bytes()[19],
                b'8' | b'9' | b'a' | b'b'
            ));
        }
    }

    #[test]
    fn vm_index_test_btoa_atob_basic_auth_portable_rows_match_upstream() {
        assert_eq!(btoa("hello world"), "aGVsbG8gd29ybGQ=");
        assert_eq!(atob("aGVsbG8gd29ybGQ=").unwrap(), "hello world");
        let header = btoa("api_key:api_secret");
        assert_eq!(header, "YXBpX2tleTphcGlfc2VjcmV0");
        assert_eq!(atob(&header).unwrap(), "api_key:api_secret");
    }
}
