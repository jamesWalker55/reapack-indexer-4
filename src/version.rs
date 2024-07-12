use std::str::FromStr;

use thiserror::Error;

#[derive(Error, Debug)]
#[error("unable to parse this version string, please specify the new version manually: {0}")]
pub(crate) struct UnknownVersionFormat(String);

pub(crate) fn increment_version(text: &str) -> Result<String, UnknownVersionFormat> {
    let text = text.to_string();

    let suffix = {
        let mut suffix = String::new();
        for c in text.chars().rev() {
            // if found non-digit char, stop the loop
            if c.is_digit(10) {
                suffix.push(c);
            } else {
                break;
            }
        }
        if suffix.is_empty() {
            return Err(UnknownVersionFormat(text.into()));
        }
        suffix = suffix.chars().rev().collect();
        Ok(suffix)
    }?;

    // Parse the suffix to an integer
    let incremented_suffix = suffix.parse::<u32>().unwrap() + 1;

    // Create the new version string
    let prefix_len = text.len() - suffix.len();

    Ok(format!("{}{}", &text[..prefix_len], incremented_suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01() {
        let result = increment_version("0.1.15").unwrap();
        let expected = "0.1.16";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_02() {
        let result = increment_version("0.1").unwrap();
        let expected = "0.2";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_03() {
        let result = increment_version("0.1a");
        assert!(result.is_err());
    }
}
