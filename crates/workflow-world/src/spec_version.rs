use serde::{Deserialize, Serialize};

/// Branded newtype for workflow world spec versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SpecVersion(u32);

impl SpecVersion {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

pub const SPEC_VERSION_LEGACY: SpecVersion = SpecVersion::new(1);
pub const SPEC_VERSION_SUPPORTS_EVENT_SOURCING: SpecVersion = SpecVersion::new(2);
pub const SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT: SpecVersion = SpecVersion::new(3);
pub const SPEC_VERSION_CURRENT: SpecVersion = SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT;

pub const fn is_legacy_spec_version(version: Option<SpecVersion>) -> bool {
    match version {
        Some(version) => version.0 <= SPEC_VERSION_LEGACY.0,
        None => true,
    }
}

pub const fn requires_newer_world(version: Option<SpecVersion>) -> bool {
    match version {
        Some(version) => version.0 > SPEC_VERSION_CURRENT.0,
        None => false,
    }
}

impl From<SpecVersion> for u32 {
    fn from(value: SpecVersion) -> Self {
        value.get()
    }
}
