use semver::Version;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentCompatibility {
    pub(crate) version: Version,
    pub(crate) protocol_min: u32,
    pub(crate) protocol_max: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComponentDecision {
    UseInstalled,
    InstallEmbedded,
    RejectNewerIncompatible,
}

/// Chooses the machine-wide source without allowing an older app to downgrade it.
pub(crate) fn decide_component(
    installed: Option<&ComponentCompatibility>,
    embedded_version: &Version,
    app_protocol: u32,
) -> ComponentDecision {
    let Some(installed) = installed else {
        return ComponentDecision::InstallEmbedded;
    };
    let compatible =
        installed.protocol_min <= app_protocol && app_protocol <= installed.protocol_max;
    if compatible && installed.version >= *embedded_version {
        ComponentDecision::UseInstalled
    } else if !compatible && installed.version > *embedded_version {
        ComponentDecision::RejectNewerIncompatible
    } else {
        ComponentDecision::InstallEmbedded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(version: &str, protocol_min: u32, protocol_max: u32) -> ComponentCompatibility {
        ComponentCompatibility {
            version: Version::parse(version).unwrap(),
            protocol_min,
            protocol_max,
        }
    }

    #[test]
    fn contract_component_selection_installs_missing_or_older_sources() {
        let embedded = Version::parse("2.0.0").unwrap();
        assert_eq!(
            decide_component(None, &embedded, 1),
            ComponentDecision::InstallEmbedded
        );
        assert_eq!(
            decide_component(Some(&component("1.9.0", 1, 1)), &embedded, 1),
            ComponentDecision::InstallEmbedded
        );
    }

    #[test]
    fn contract_component_selection_keeps_compatible_equal_or_newer_sources() {
        let embedded = Version::parse("2.0.0").unwrap();
        assert_eq!(
            decide_component(Some(&component("2.0.0", 1, 1)), &embedded, 1),
            ComponentDecision::UseInstalled
        );
        assert_eq!(
            decide_component(Some(&component("3.0.0", 1, 2)), &embedded, 1),
            ComponentDecision::UseInstalled
        );
    }

    #[test]
    fn contract_component_selection_rejects_only_newer_incompatible_sources() {
        let embedded = Version::parse("2.0.0").unwrap();
        assert_eq!(
            decide_component(Some(&component("3.0.0", 2, 2)), &embedded, 1),
            ComponentDecision::RejectNewerIncompatible
        );
        assert_eq!(
            decide_component(Some(&component("2.0.0", 2, 2)), &embedded, 1),
            ComponentDecision::InstallEmbedded
        );
    }
}
