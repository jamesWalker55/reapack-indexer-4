use itertools::Itertools;
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
            if c.is_ascii_digit() {
                suffix.push(c);
            } else {
                break;
            }
        }
        if suffix.is_empty() {
            return Err(UnknownVersionFormat(text));
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

pub(crate) fn find_latest_version<'a, I>(versions: I) -> Option<&'a str>
where
    I: Iterator<Item = &'a str>,
{
    versions.max_by(|version_a, version_b| {
        for entry in version_a.split('.').zip_longest(version_b.split('.')) {
            match entry {
                itertools::EitherOrBoth::Both(part_a, part_b) => match part_a
                    .partial_cmp(part_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                {
                    // comparison is equal, don't return, keep iterating
                    std::cmp::Ordering::Equal => (),
                    // otherwise, return that order (greater/less)
                    order => return order,
                },
                // if one version is longer, return that one
                itertools::EitherOrBoth::Left(_part_a) => return std::cmp::Ordering::Greater,
                itertools::EitherOrBoth::Right(_part_b) => return std::cmp::Ordering::Less,
            };
        }
        std::cmp::Ordering::Equal
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_increment_01() {
        let result = increment_version("0.1.15").unwrap();
        let expected = "0.1.16";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_increment_02() {
        let result = increment_version("0.1").unwrap();
        let expected = "0.2";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_increment_03() {
        let result = increment_version("0.1a");
        assert!(result.is_err());
    }

    #[test]
    fn test_latest_01() {
        let result = find_latest_version(vec!["0.1.0", "0.1.15"].into_iter()).unwrap();
        let expected = "0.1.15";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_latest_02() {
        let result = find_latest_version(vec!["0.1.1", "0.1.15"].into_iter()).unwrap();
        let expected = "0.1.15";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_latest_03() {
        let result = find_latest_version(vec!["0.1", "0.1.15"].into_iter()).unwrap();
        let expected = "0.1.15";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_latest_04() {
        let result = find_latest_version(vec!["0.1.15", "0.1.15"].into_iter()).unwrap();
        let expected = "0.1.15";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_latest_05() {
        let result = find_latest_version(vec!["0.1.15b", "0.1.15"].into_iter()).unwrap();
        let expected = "0.1.15b";
        assert_eq!(result, expected);
    }
}
