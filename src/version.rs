use std::str::FromStr;

use thiserror::Error;

#[derive(Error, Debug)]
#[error("unable to parse this version string: {0}")]
pub(crate) struct UnknownVersionFormat(String);

pub(crate) struct Version {
    a: u32,
    b: u32,
    c: u32,
}

impl Version {
    pub(crate) fn increment(&self) -> Self {
        Self {
            a: self.a,
            b: self.b,
            c: self.c + 1,
        }
    }
}

impl Default for Version {
    fn default() -> Self {
        Self { a: 0, b: 0, c: 1 }
    }
}

impl FromStr for Version {
    type Err = UnknownVersionFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(".");

        let a = {
            let text = parts
                .next()
                .ok_or_else(|| UnknownVersionFormat(s.to_string()))?;
            let num = text
                .parse::<u32>()
                .map_err(|_| UnknownVersionFormat(s.to_string()))?;
            Ok(num)
        }?;
        let b = {
            let text = parts
                .next()
                .ok_or_else(|| UnknownVersionFormat(s.to_string()))?;
            let num = text
                .parse::<u32>()
                .map_err(|_| UnknownVersionFormat(s.to_string()))?;
            Ok(num)
        }?;
        let c = {
            let text = parts
                .next()
                .ok_or_else(|| UnknownVersionFormat(s.to_string()))?;
            let num = text
                .parse::<u32>()
                .map_err(|_| UnknownVersionFormat(s.to_string()))?;
            Ok(num)
        }?;

        if parts.next().is_some() {
            return Err(UnknownVersionFormat(s.to_string()));
        }

        Ok(Self { a, b, c })
    }
}

impl From<&Version> for String {
    fn from(ver: &Version) -> Self {
        format!("{}.{}.{}", ver.a, ver.b, ver.c)
    }
}
