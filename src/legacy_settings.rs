//! Settings read from the environment were named `GAH_*` before the product
//! became Parasaur. Every setting is now `PARASAUR_*`. An installed node still
//! carries the old names in its unit files, launch agents and env files, so
//! each binary calls [`adopt`] once at startup. Nothing else in the crate knows
//! the old prefix.
//!
//! Delete this module and its callers once installed nodes have been rewritten.

use std::ffi::OsString;

const LEGACY: &str = "GAH_";
const CURRENT: &str = "PARASAUR_";

/// Gives every setting that is present only under its old name its current
/// name too. A current name that is already set wins. The old variable stays,
/// so anything outside this process that reads it keeps working.
///
/// Call before the first setting is read and before any thread starts.
pub fn adopt() {
    for (name, value) in adopted(std::env::vars_os()) {
        std::env::set_var(name, value);
    }
}

/// The old spelling of a current setting name, for reading a file that an
/// older install wrote. `None` when `current` is not a Parasaur setting.
pub fn legacy_name(current: &str) -> Option<String> {
    current
        .strip_prefix(CURRENT)
        .map(|rest| format!("{LEGACY}{rest}"))
}

fn adopted(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<(String, OsString)> {
    let vars: Vec<(OsString, OsString)> = vars.into_iter().collect();
    vars.iter()
        .filter_map(|(name, value)| {
            let current = format!("{CURRENT}{}", name.to_str()?.strip_prefix(LEGACY)?);
            let taken = vars
                .iter()
                .any(|(other, _)| other.to_str() == Some(&current));
            (!taken).then(|| (current, value.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    #[test]
    fn an_old_name_is_adopted_unless_the_current_name_is_set() {
        let result = adopted(vars(&[
            ("GAH_CONFIG", "/old/config.toml"),
            ("GAH_BINARY", "/old/gah"),
            ("PARASAUR_BINARY", "/new/parasaur"),
            ("HOME", "/home/someone"),
            ("AGAH_CONFIG", "unrelated"),
        ]));
        assert_eq!(
            result,
            vec![(
                "PARASAUR_CONFIG".to_string(),
                OsString::from("/old/config.toml")
            )]
        );
    }

    #[test]
    fn a_current_name_maps_back_to_its_old_spelling() {
        assert_eq!(
            legacy_name("PARASAUR_TAILSCALE_SERVE").as_deref(),
            Some("GAH_TAILSCALE_SERVE")
        );
        assert_eq!(legacy_name("PORT"), None);
    }
}
