use std::collections::BTreeSet;

/// Well-known serialization format prefixes from upstream `serialization`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum SerializationFormat {
    DevalueV1,
    Encrypted,
}

impl SerializationFormat {
    #[must_use]
    pub const fn as_prefix(self) -> &'static str {
        match self {
            Self::DevalueV1 => "devl",
            Self::Encrypted => "encr",
        }
    }
}

/// Capabilities of a workflow run based on its `@workflow/core` version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunCapabilities {
    pub supported_formats: BTreeSet<SerializationFormat>,
}

/// Look up serialization formats supported by a run's `@workflow/core` version.
#[must_use]
pub fn get_run_capabilities(workflow_core_version: Option<&str>) -> RunCapabilities {
    let mut supported_formats = BTreeSet::from([SerializationFormat::DevalueV1]);

    if let Some(version) = workflow_core_version.and_then(Semver::parse) {
        let encryption_cutoff =
            Semver::parse("4.2.0-beta.64").expect("static workflow semver cutoff should parse");
        if version >= encryption_cutoff {
            supported_formats.insert(SerializationFormat::Encrypted);
        }
    }

    RunCapabilities { supported_formats }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Semver {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<Vec<Identifier>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Identifier {
    Numeric(u64),
    AlphaNumeric(String),
}

impl Semver {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.strip_prefix('v').unwrap_or(raw);
        let (version, prerelease) = match raw.split_once('-') {
            Some((version, prerelease)) => (version, Some(parse_prerelease(prerelease)?)),
            None => (raw, None),
        };

        let mut parts = version.split('.');
        let major = parse_numeric(parts.next()?)?;
        let minor = parse_numeric(parts.next()?)?;
        let patch = parse_numeric(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }

        Some(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }
}

impl PartialOrd for Semver {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Semver {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| compare_prerelease(&self.prerelease, &other.prerelease))
    }
}

fn parse_numeric(raw: &str) -> Option<u64> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

fn parse_prerelease(raw: &str) -> Option<Vec<Identifier>> {
    if raw.is_empty() {
        return None;
    }
    raw.split('.')
        .map(|part| {
            if part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return None;
            }
            if part.bytes().all(|byte| byte.is_ascii_digit()) {
                Some(Identifier::Numeric(part.parse().ok()?))
            } else {
                Some(Identifier::AlphaNumeric(part.to_string()))
            }
        })
        .collect()
}

fn compare_prerelease(
    left: &Option<Vec<Identifier>>,
    right: &Option<Vec<Identifier>>,
) -> std::cmp::Ordering {
    match (left, right) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(left), Some(right)) => {
            for (left, right) in left.iter().zip(right) {
                let ordering = match (left, right) {
                    (Identifier::Numeric(left), Identifier::Numeric(right)) => left.cmp(right),
                    (Identifier::Numeric(_), Identifier::AlphaNumeric(_)) => {
                        std::cmp::Ordering::Less
                    }
                    (Identifier::AlphaNumeric(_), Identifier::Numeric(_)) => {
                        std::cmp::Ordering::Greater
                    }
                    (Identifier::AlphaNumeric(left), Identifier::AlphaNumeric(right)) => {
                        left.cmp(right)
                    }
                };
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
            left.len().cmp(&right.len())
        }
    }
}
