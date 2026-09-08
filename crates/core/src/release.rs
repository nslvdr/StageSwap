//! Version contract used by release publishing and update selection.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReleaseVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl ReleaseVersion {
    pub fn parse(value: &str) -> Result<Self, ReleaseVersionError> {
        let value = value.strip_prefix('v').unwrap_or(value);
        let mut parts = value.split('.');
        let version = Self {
            major: parts
                .next()
                .ok_or(ReleaseVersionError("missing major version"))?
                .parse()
                .map_err(|_| ReleaseVersionError("invalid major version"))?,
            minor: parts
                .next()
                .ok_or(ReleaseVersionError("missing minor version"))?
                .parse()
                .map_err(|_| ReleaseVersionError("invalid minor version"))?,
            patch: parts
                .next()
                .ok_or(ReleaseVersionError("missing patch version"))?
                .parse()
                .map_err(|_| ReleaseVersionError("invalid patch version"))?,
        };
        if parts.next().is_some() {
            return Err(ReleaseVersionError(
                "version must contain exactly three numbers",
            ));
        }
        Ok(version)
    }
}

impl std::fmt::Display for ReleaseVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl ReleaseVersion {
    pub fn increment_patch(self) -> Result<Self, ReleaseVersionError> {
        Ok(Self {
            patch: self
                .patch
                .checked_add(1)
                .ok_or(ReleaseVersionError("patch version overflow"))?,
            ..self
        })
    }

    pub fn increment_minor(self) -> Result<Self, ReleaseVersionError> {
        Ok(Self {
            minor: self
                .minor
                .checked_add(1)
                .ok_or(ReleaseVersionError("minor version overflow"))?,
            patch: 0,
            ..self
        })
    }

    pub fn increment_major(self) -> Result<Self, ReleaseVersionError> {
        Ok(Self {
            major: self
                .major
                .checked_add(1)
                .ok_or(ReleaseVersionError("major version overflow"))?,
            minor: 0,
            patch: 0,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleaseVersionError(&'static str);

impl std::fmt::Display for ReleaseVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ReleaseVersionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_release_versions_round_trip_and_sort_numerically() {
        let version = ReleaseVersion::parse("v1.9.3").unwrap();
        assert_eq!(version.to_string(), "1.9.3");
        assert_eq!(ReleaseVersion::parse(&version.to_string()), Ok(version));
        assert!(version < ReleaseVersion::parse("1.10.0").unwrap());
        for invalid in ["", "1", "1.2", "1.2.3.4", "1.2.beta", "1.2.3-beta"] {
            assert!(ReleaseVersion::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn contract_version_bumps_reset_lower_components_and_reject_overflow() {
        let version = ReleaseVersion::parse("1.9.3").unwrap();
        assert_eq!(version.increment_patch().unwrap().to_string(), "1.9.4");
        assert_eq!(version.increment_minor().unwrap().to_string(), "1.10.0");
        assert_eq!(version.increment_major().unwrap().to_string(), "2.0.0");
        let maximum = ReleaseVersion {
            major: u64::MAX,
            minor: u64::MAX,
            patch: u64::MAX,
        };
        assert!(maximum.increment_patch().is_err());
        assert!(maximum.increment_minor().is_err());
        assert!(maximum.increment_major().is_err());
    }
}
