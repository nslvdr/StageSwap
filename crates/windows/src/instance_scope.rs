/// Builds account-scoped names for resources shared across Windows sessions.
pub(crate) fn application_mutex_name(user_sid: &str) -> String {
    format!(r"Global\StageSwap.Application.{user_sid}")
}

pub(crate) fn install_mutex_name(user_sid: &str) -> String {
    format!(r"Global\StageSwap.Install.{user_sid}")
}

pub(crate) fn control_pipe_name(user_sid: &str) -> String {
    format!(r"\\.\pipe\StageSwap.Control.{user_sid}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_account_scoped_names_are_stable_and_distinct() {
        let first = "S-1-5-21-1000";
        let second = "S-1-5-21-2000";

        assert_eq!(
            application_mutex_name(first),
            r"Global\StageSwap.Application.S-1-5-21-1000"
        );
        assert_eq!(
            install_mutex_name(first),
            r"Global\StageSwap.Install.S-1-5-21-1000"
        );
        assert_eq!(
            control_pipe_name(first),
            r"\\.\pipe\StageSwap.Control.S-1-5-21-1000"
        );
        assert_ne!(control_pipe_name(first), control_pipe_name(second));
    }
}
