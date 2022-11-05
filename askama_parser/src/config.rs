//! Askama parser configuration.
//!
//! This module handles the configuration format for Askama.
//! Load a `Config` object by calling `from_file`, pass `None`
//! to load the project's default `askama.toml`.
//!
//! ```no_run
//! use askama_parser::config::Config;
//!
//! let default_config = Config::from_file(None)
//!     .expect("load config");
//! ```

use std::collections::{BTreeMap, HashSet};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::{env, fs};

#[cfg(feature = "serde")]
use serde::Deserialize;

use crate::CompileError;

/// Askama parser configuration.
#[derive(Debug)]
pub struct Config {
    dirs: Vec<PathBuf>,
    pub(crate) syntaxes: BTreeMap<String, Syntax>,
    pub(crate) default_syntax: String,
    pub(crate) escapers: Vec<(HashSet<String>, String)>,
    whitespace: WhitespaceHandling,
}

impl Config {
    /// Load Askama configuration from the project's config file.
    ///
    /// This will try to load TOML file with Askama configuration
    /// for the dependent project.  The config file is relative
    /// to `CARGO_MANIFEST_DIR`.  If a filename is not provided,
    /// it defaults to `askama.toml`.
    pub fn from_file(file: Option<&str>) -> std::result::Result<Config, CompileError> {
        let config_toml = read_config_file(file)?;
        Config::from_toml(&config_toml)
    }

    /// Load Askama configuration from TOML source.
    pub fn from_toml(s: &str) -> std::result::Result<Config, CompileError> {
        let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let default_dirs = vec![root.join("templates")];

        let mut syntaxes = BTreeMap::new();
        syntaxes.insert(DEFAULT_SYNTAX_NAME.to_string(), Syntax::default());

        let raw = if s.is_empty() {
            RawConfig::default()
        } else {
            RawConfig::from_toml_str(s)?
        };

        let (dirs, default_syntax, whitespace) = match raw.general {
            Some(General {
                dirs,
                default_syntax,
                whitespace,
            }) => (
                dirs.map_or(default_dirs, |v| {
                    v.into_iter().map(|dir| root.join(dir)).collect()
                }),
                default_syntax.unwrap_or(DEFAULT_SYNTAX_NAME),
                whitespace,
            ),
            None => (
                default_dirs,
                DEFAULT_SYNTAX_NAME,
                WhitespaceHandling::default(),
            ),
        };

        if let Some(raw_syntaxes) = raw.syntax {
            for raw_s in raw_syntaxes {
                let name = raw_s.name;

                if syntaxes
                    .insert(name.to_string(), Syntax::try_from(raw_s)?)
                    .is_some()
                {
                    return Err(format!("syntax \"{}\" is already defined", name).into());
                }
            }
        }

        if !syntaxes.contains_key(default_syntax) {
            return Err(format!("default syntax \"{}\" not found", default_syntax).into());
        }

        let mut escapers = Vec::new();
        if let Some(configured) = raw.escaper {
            for escaper in configured {
                escapers.push((
                    escaper
                        .extensions
                        .iter()
                        .map(|ext| (*ext).to_string())
                        .collect(),
                    escaper.path.to_string(),
                ));
            }
        }
        for (extensions, path) in DEFAULT_ESCAPERS {
            escapers.push((str_set(extensions), (*path).to_string()));
        }

        Ok(Config {
            dirs,
            syntaxes,
            default_syntax: default_syntax.into(),
            escapers,
            whitespace,
        })
    }

    /// Find a template file based on this configuration.
    pub fn find_template(
        &self,
        path: &str,
        start_at: Option<&Path>,
    ) -> std::result::Result<PathBuf, CompileError> {
        if let Some(root) = start_at {
            let relative = root.with_file_name(path);
            if relative.exists() {
                return Ok(relative);
            }
        }

        for dir in &self.dirs {
            let rooted = dir.join(path);
            if rooted.exists() {
                return Ok(rooted);
            }
        }

        Err(format!(
            "template {:?} not found in directories {:?}",
            path, self.dirs
        )
        .into())
    }

    /// Find the escaper to use for the given content type.
    pub fn find_escaper(&self, name: &str) -> Option<&str> {
        self.escapers
            .iter()
            .find_map(|(escapers, escaper)| escapers.contains(name).then_some(escaper.as_ref()))
    }

    /// The whitespace handling to use.
    pub fn whitespace(&self) -> WhitespaceHandling {
        self.whitespace
    }
}

/// The definition of a custom template syntax.
#[derive(Debug)]
pub struct Syntax {
    /// Defaults to `"{%"`.
    pub block_start: String,
    /// Defaults to `"%}"`.
    pub block_end: String,
    /// Defaults to `"{{"`.
    pub expr_start: String,
    /// Defaults to `"}}"`.
    pub expr_end: String,
    /// Defaults to `"{#"`.
    pub comment_start: String,
    /// Defaults to `"#}"`.
    pub comment_end: String,
}

impl Default for Syntax {
    fn default() -> Self {
        Self {
            block_start: "{%".into(),
            block_end: "%}".into(),
            expr_start: "{{".into(),
            expr_end: "}}".into(),
            comment_start: "{#".into(),
            comment_end: "#}".into(),
        }
    }
}

impl<'a> TryFrom<RawSyntax<'a>> for Syntax {
    type Error = CompileError;

    fn try_from(raw: RawSyntax<'a>) -> std::result::Result<Self, Self::Error> {
        let default = Self::default();
        let syntax = Self {
            block_start: raw.block_start.map(ToString::to_string).unwrap_or(default.block_start),
            block_end: raw.block_end.map(ToString::to_string).unwrap_or(default.block_end),
            expr_start: raw.expr_start.map(ToString::to_string).unwrap_or(default.expr_start),
            expr_end: raw.expr_end.map(ToString::to_string).unwrap_or(default.expr_end),
            comment_start: raw.comment_start.map(ToString::to_string).unwrap_or(default.comment_start),
            comment_end: raw.comment_end.map(ToString::to_string).unwrap_or(default.comment_end),
        };

        if syntax.block_start.len() != 2
            || syntax.block_end.len() != 2
            || syntax.expr_start.len() != 2
            || syntax.expr_end.len() != 2
            || syntax.comment_start.len() != 2
            || syntax.comment_end.len() != 2
        {
            return Err("length of delimiters must be two".into());
        }

        let bs = syntax.block_start.as_bytes()[0];
        let be = syntax.block_start.as_bytes()[1];
        let cs = syntax.comment_start.as_bytes()[0];
        let ce = syntax.comment_start.as_bytes()[1];
        let es = syntax.expr_start.as_bytes()[0];
        let ee = syntax.expr_start.as_bytes()[1];
        if !((bs == cs && bs == es) || (be == ce && be == ee)) {
            return Err(format!("bad delimiters block_start: {}, comment_start: {}, expr_start: {}, needs one of the two characters in common", syntax.block_start, syntax.comment_start, syntax.expr_start).into());
        }

        Ok(syntax)
    }
}

#[cfg_attr(feature = "serde", derive(Deserialize))]
#[derive(Default)]
struct RawConfig<'d> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    general: Option<General<'d>>,
    syntax: Option<Vec<RawSyntax<'d>>>,
    escaper: Option<Vec<RawEscaper<'d>>>,
}

impl RawConfig<'_> {
    #[cfg(feature = "config")]
    fn from_toml_str(s: &str) -> std::result::Result<RawConfig<'_>, CompileError> {
        toml::from_str(s).map_err(|e| format!("invalid TOML in {}: {}", CONFIG_FILE_NAME, e).into())
    }

    #[cfg(not(feature = "config"))]
    fn from_toml_str(_: &str) -> std::result::Result<RawConfig<'_>, CompileError> {
        Err("TOML support not available".into())
    }
}

/// How should we handle whitespace in the template?
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
#[cfg_attr(feature = "serde", serde(field_identifier, rename_all = "lowercase"))]
pub enum WhitespaceHandling {
    /// The default behaviour. It will leave the whitespace characters "as is".
    Preserve,
    /// It'll remove all the whitespace characters before and after the jinja block.
    Suppress,
    /// It'll remove all the whitespace characters except one before and after the jinja blocks.
    /// If there is a newline character, the preserved character in the trimmed characters, it will
    /// the one preserved.
    Minimize,
}

impl Default for WhitespaceHandling {
    fn default() -> Self {
        WhitespaceHandling::Preserve
    }
}

#[cfg_attr(feature = "serde", derive(Deserialize))]
struct General<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    dirs: Option<Vec<&'a str>>,
    default_syntax: Option<&'a str>,
    #[cfg_attr(feature = "serde", serde(default))]
    whitespace: WhitespaceHandling,
}

#[cfg_attr(feature = "serde", derive(Deserialize))]
struct RawSyntax<'a> {
    name: &'a str,
    block_start: Option<&'a str>,
    block_end: Option<&'a str>,
    expr_start: Option<&'a str>,
    expr_end: Option<&'a str>,
    comment_start: Option<&'a str>,
    comment_end: Option<&'a str>,
}

#[cfg_attr(feature = "serde", derive(Deserialize))]
struct RawEscaper<'a> {
    path: &'a str,
    extensions: Vec<&'a str>,
}

fn read_config_file(config_path: Option<&str>) -> std::result::Result<String, CompileError> {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let filename = match config_path {
        Some(config_path) => root.join(config_path),
        None => root.join(CONFIG_FILE_NAME),
    };

    if filename.exists() {
        fs::read_to_string(&filename)
            .map_err(|_| format!("unable to read {:?}", filename.to_str().unwrap()).into())
    } else if config_path.is_some() {
        Err(format!("`{}` does not exist", root.display()).into())
    } else {
        Ok("".to_string())
    }
}

fn str_set<T>(vals: &[T]) -> HashSet<String>
where
    T: ToString,
{
    vals.iter().map(|s| s.to_string()).collect()
}

/// Load a template file to a string.
#[allow(clippy::match_wild_err_arm)]
pub fn get_template_source(tpl_path: &Path) -> std::result::Result<String, CompileError> {
    match fs::read_to_string(tpl_path) {
        Err(_) => Err(format!(
            "unable to open template file '{}'",
            tpl_path.to_str().unwrap()
        )
        .into()),
        Ok(mut source) => {
            if source.ends_with('\n') {
                let _ = source.pop();
            }
            Ok(source)
        }
    }
}

static CONFIG_FILE_NAME: &str = "askama.toml";
static DEFAULT_SYNTAX_NAME: &str = "default";
static DEFAULT_ESCAPERS: &[(&[&str], &str)] = &[
    (&["html", "htm", "xml"], "::askama::Html"),
    (&["md", "none", "txt", "yml", ""], "::askama::Text"),
    (&["j2", "jinja", "jinja2"], "::askama::Html"),
];

#[cfg(test)]
#[allow(clippy::blacklisted_name)]
mod tests {
    use std::env;
    use std::path::{Path, PathBuf};

    use super::*;

    #[test]
    fn get_source() {
        let path = Config::from_toml("")
            .and_then(|config| config.find_template("b.html", None))
            .unwrap();
        assert_eq!(get_template_source(&path).unwrap(), "bar");
    }

    #[test]
    fn test_default_config() {
        let mut root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        root.push("templates");
        let config = Config::from_toml("").unwrap();
        assert_eq!(config.dirs, vec![root]);
    }

    #[cfg(feature = "config")]
    #[test]
    fn test_config_dirs() {
        let mut root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        root.push("tpl");
        let config = Config::from_toml("[general]\ndirs = [\"tpl\"]").unwrap();
        assert_eq!(config.dirs, vec![root]);
    }

    fn assert_eq_rooted(actual: &Path, expected: &str) {
        let mut root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        root.push("templates");
        let mut inner = PathBuf::new();
        inner.push(expected);
        assert_eq!(actual.strip_prefix(root).unwrap(), inner);
    }

    #[test]
    fn find_absolute() {
        let config = Config::from_toml("").unwrap();
        let root = config.find_template("a.html", None).unwrap();
        let path = config.find_template("sub/b.html", Some(&root)).unwrap();
        assert_eq_rooted(&path, "sub/b.html");
    }

    #[test]
    #[should_panic]
    fn find_relative_nonexistent() {
        let config = Config::from_toml("").unwrap();
        let root = config.find_template("a.html", None).unwrap();
        config.find_template("c.html", Some(&root)).unwrap();
    }

    #[test]
    fn find_relative() {
        let config = Config::from_toml("").unwrap();
        let root = config.find_template("sub/b.html", None).unwrap();
        let path = config.find_template("c.html", Some(&root)).unwrap();
        assert_eq_rooted(&path, "sub/c.html");
    }

    #[test]
    fn find_relative_sub() {
        let config = Config::from_toml("").unwrap();
        let root = config.find_template("sub/b.html", None).unwrap();
        let path = config.find_template("sub1/d.html", Some(&root)).unwrap();
        assert_eq_rooted(&path, "sub/sub1/d.html");
    }

    #[cfg(feature = "config")]
    #[test]
    fn add_syntax() {
        let raw_config = r#"
        [general]
        default_syntax = "foo"

        [[syntax]]
        name = "foo"
        block_start = "{<"

        [[syntax]]
        name = "bar"
        expr_start = "{!"
        "#;

        let default_syntax = Syntax::default();
        let config = Config::from_toml(raw_config).unwrap();
        assert_eq!(config.default_syntax, "foo");

        let foo = config.syntaxes.get("foo").unwrap();
        assert_eq!(foo.block_start, "{<");
        assert_eq!(foo.block_end, default_syntax.block_end);
        assert_eq!(foo.expr_start, default_syntax.expr_start);
        assert_eq!(foo.expr_end, default_syntax.expr_end);
        assert_eq!(foo.comment_start, default_syntax.comment_start);
        assert_eq!(foo.comment_end, default_syntax.comment_end);

        let bar = config.syntaxes.get("bar").unwrap();
        assert_eq!(bar.block_start, default_syntax.block_start);
        assert_eq!(bar.block_end, default_syntax.block_end);
        assert_eq!(bar.expr_start, "{!");
        assert_eq!(bar.expr_end, default_syntax.expr_end);
        assert_eq!(bar.comment_start, default_syntax.comment_start);
        assert_eq!(bar.comment_end, default_syntax.comment_end);
    }

    #[cfg(feature = "config")]
    #[test]
    fn add_syntax_two() {
        let raw_config = r#"
        syntax = [{ name = "foo", block_start = "{<" },
                  { name = "bar", expr_start = "{!" } ]

        [general]
        default_syntax = "foo"
        "#;

        let default_syntax = Syntax::default();
        let config = Config::from_toml(raw_config).unwrap();
        assert_eq!(config.default_syntax, "foo");

        let foo = config.syntaxes.get("foo").unwrap();
        assert_eq!(foo.block_start, "{<");
        assert_eq!(foo.block_end, default_syntax.block_end);
        assert_eq!(foo.expr_start, default_syntax.expr_start);
        assert_eq!(foo.expr_end, default_syntax.expr_end);
        assert_eq!(foo.comment_start, default_syntax.comment_start);
        assert_eq!(foo.comment_end, default_syntax.comment_end);

        let bar = config.syntaxes.get("bar").unwrap();
        assert_eq!(bar.block_start, default_syntax.block_start);
        assert_eq!(bar.block_end, default_syntax.block_end);
        assert_eq!(bar.expr_start, "{!");
        assert_eq!(bar.expr_end, default_syntax.expr_end);
        assert_eq!(bar.comment_start, default_syntax.comment_start);
        assert_eq!(bar.comment_end, default_syntax.comment_end);
    }

    #[cfg(feature = "toml")]
    #[should_panic]
    #[test]
    fn use_default_at_syntax_name() {
        let raw_config = r#"
        syntax = [{ name = "default" }]
        "#;

        let _config = Config::from_toml(raw_config).unwrap();
    }

    #[cfg(feature = "toml")]
    #[should_panic]
    #[test]
    fn duplicated_syntax_name_on_list() {
        let raw_config = r#"
        syntax = [{ name = "foo", block_start = "~<" },
                  { name = "foo", block_start = "%%" } ]
        "#;

        let _config = Config::from_toml(raw_config).unwrap();
    }

    #[cfg(feature = "toml")]
    #[should_panic]
    #[test]
    fn is_not_exist_default_syntax() {
        let raw_config = r#"
        [general]
        default_syntax = "foo"
        "#;

        let _config = Config::from_toml(raw_config).unwrap();
    }

    #[cfg(feature = "config")]
    #[test]
    fn escape_modes() {
        let config = Config::from_toml(
            r#"
            [[escaper]]
            path = "::askama::Js"
            extensions = ["js"]
        "#,
        )
        .unwrap();
        assert_eq!(
            config.escapers,
            vec![
                (str_set(&["js"]), "::askama::Js".into()),
                (str_set(&["html", "htm", "xml"]), "::askama::Html".into()),
                (
                    str_set(&["md", "none", "txt", "yml", ""]),
                    "::askama::Text".into()
                ),
                (str_set(&["j2", "jinja", "jinja2"]), "::askama::Html".into()),
            ]
        );
    }

    #[cfg(feature = "config")]
    #[test]
    fn test_whitespace_parsing() {
        let config = Config::from_toml(
            r#"
            [general]
            whitespace = "suppress"
            "#,
        )
        .unwrap();
        assert_eq!(config.whitespace, WhitespaceHandling::Suppress);

        let config = Config::from_toml(r#""#).unwrap();
        assert_eq!(config.whitespace, WhitespaceHandling::Preserve);

        let config = Config::from_toml(
            r#"
            [general]
            whitespace = "preserve"
            "#,
        )
        .unwrap();
        assert_eq!(config.whitespace, WhitespaceHandling::Preserve);

        let config = Config::from_toml(
            r#"
            [general]
            whitespace = "minimize"
            "#,
        )
        .unwrap();
        assert_eq!(config.whitespace, WhitespaceHandling::Minimize);
    }
}
