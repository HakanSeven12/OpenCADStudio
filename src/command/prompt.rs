//! The prompts every variable asks its value with, through one catalog
//! entry each so they are translated everywhere.

/// `Enter new value for NAME <current>:`, the one prompt every variable asks
/// its value with, through one catalog entry so it is translated everywhere.
pub fn value_prompt(name: &str, current: &dyn std::fmt::Display) -> String {
    let current = current.to_string();
    crate::tf!("Enter new value for {name} <{current}>:").into_owned()
}

/// [`value_prompt`] for a name-valued variable that `.` clears.
pub fn value_prompt_or_none(name: &str, current: &dyn std::fmt::Display) -> String {
    let current = current.to_string();
    crate::tf!("Enter new value for {name}, or . for none <\"{current}\">:").into_owned()
}

/// [`value_prompt`] for a variable with no value to offer.
pub fn value_prompt_bare(name: &str) -> String {
    crate::tf!("Enter new value for {name}:").into_owned()
}
