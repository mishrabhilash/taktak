//! A tiny `--option value` parser. The CLI is small enough that a full parser crate would
//! cost more than it saves.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

pub struct Args {
    values: BTreeMap<String, String>,
    flags: BTreeSet<String>,
}

impl Args {
    /// Parses `args`. `valued` lists options that take a value (`--out DIR` or `--out=DIR`),
    /// `flags` the ones that do not. Anything else is an error.
    pub fn parse(
        args: impl IntoIterator<Item = String>,
        valued: &[&str],
        flags: &[&str],
    ) -> Result<Args, String> {
        let mut parsed = Args { values: BTreeMap::new(), flags: BTreeSet::new() };
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n.to_owned(), Some(v.to_owned())),
                _ => (arg.clone(), None),
            };
            if valued.contains(&name.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or_else(|| format!("{name} needs a value"))?,
                };
                if parsed.values.insert(name.clone(), value).is_some() {
                    return Err(format!("{name} given more than once"));
                }
            } else if flags.contains(&name.as_str()) && inline.is_none() {
                parsed.flags.insert(name);
            } else if name.starts_with('-') {
                return Err(format!("unknown option {name}"));
            } else {
                return Err(format!("unexpected argument {name:?}"));
            }
        }
        Ok(parsed)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    pub fn opt(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn req(&self, name: &str) -> Result<&str, String> {
        self.opt(name).ok_or_else(|| format!("{name} is required"))
    }

    pub fn num<T: FromStr>(&self, name: &str) -> Result<Option<T>, String> {
        self.opt(name)
            .map(|v| v.parse().map_err(|_| format!("{name}: {v:?} is not a valid number")))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|s| s.to_string()), &["--out", "--takes"], &["--force"])
    }

    #[test]
    fn values_flags_and_equals_syntax() {
        let a = parse(&["--out", "dir", "--takes=4", "--force"]).unwrap();
        assert_eq!(a.opt("--out"), Some("dir"));
        assert_eq!(a.num::<usize>("--takes").unwrap(), Some(4));
        assert!(a.flag("--force"));
        assert_eq!(a.num::<usize>("--missing").unwrap(), None);
    }

    #[test]
    fn negative_values_are_values() {
        let a = Args::parse(["--db".to_string(), "-40".to_string()], &["--db"], &[]).unwrap();
        assert_eq!(a.num::<f32>("--db").unwrap(), Some(-40.0));
    }

    #[test]
    fn errors() {
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["stray"]).is_err());
        assert!(parse(&["--out"]).is_err());
        assert!(parse(&["--out", "a", "--out", "b"]).is_err());
        assert!(parse(&["--force=yes"]).is_err());
        assert!(parse(&["--takes", "x"]).unwrap().num::<usize>("--takes").is_err());
        assert!(parse(&[]).unwrap().req("--out").is_err());
    }
}
