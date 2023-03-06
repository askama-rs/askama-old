//! Parser and syntax tree for Askama's template syntax.
//!
//! Askama template source is parsed into a [`node::Block`](./node/struct.Block.html),
//! which contains a sequence of [`node::Node`s](./node/enum.Node.html).
//! Each `Node` represents either a bit of literal text or one of three types of
//! template tags: comments, expressions, or statements.  In turn, statements
//! can contain nested `Block`s, which form a hierarchical structure.
//!
//! The main entry point to this crate is the [`parse()`](./fn.parse.html)
//! method, which takes the template input `&str` and the configurable
//! [`syntax::Syntax`](./syntax/struct.Syntax.html) to use for parsing.

use std::cell::Cell;
use std::str;

use nom::branch::alt;
use nom::bytes::complete::{escaped, is_not, tag, take_till};
use nom::character::complete::char;
use nom::character::complete::{anychar, digit1};
use nom::combinator::{all_consuming, complete, eof, map, not, opt, recognize, value};
use nom::error::ErrorKind;
use nom::multi::separated_list1;
use nom::sequence::{delimited, pair, tuple};
use nom::{error_position, AsChar, IResult, InputTakeAtPosition};

pub(crate) use self::expr::Expr;
pub(crate) use self::node::{
    Block, BlockDef, Call, Cond, CondTest, Lit, Loop, Macro, Match, Node, Raw, Tag, Target, When,
};

mod expr;
mod node;
#[cfg(test)]
mod tests;

/// Askama template syntax configuration.
#[derive(Debug)]
pub(crate) struct Syntax<'a> {
    /// Defaults to `"{%"`.
    pub(crate) block_start: &'a str,
    /// Defaults to `"%}"`.
    pub(crate) block_end: &'a str,
    /// Defaults to `"{{"`.
    pub(crate) expr_start: &'a str,
    /// Defaults to `"}}"`.
    pub(crate) expr_end: &'a str,
    /// Defaults to `"{#"`.
    pub(crate) comment_start: &'a str,
    /// Defaults to `"#}"`.
    pub(crate) comment_end: &'a str,
}

impl Default for Syntax<'static> {
    fn default() -> Self {
        Self {
            block_start: "{%",
            block_end: "%}",
            expr_start: "{{",
            expr_end: "}}",
            comment_start: "{#",
            comment_end: "#}",
        }
    }
}

/// Whitespace preservation or suppression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Whitespace {
    Preserve,
    Suppress,
    Minimize,
}

/// Whitespace suppression for a `Tag` or `Block`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Ws {
    /// Handling of trailing whitespace on literal text at a transition in to Askama.
    pub(crate) flush: Option<Whitespace>,
    /// Handling of leading whitespace on literal text at a transition out of Askama.
    pub(crate) prepare: Option<Whitespace>,
}

impl Ws {
    // internal shorthand form, not meant to be public
    fn new(flush: Option<Whitespace>, prepare: Option<Whitespace>) -> Self {
        Ws { flush, prepare }
    }
}

struct State<'a> {
    syntax: &'a Syntax<'a>,
    loop_depth: Cell<usize>,
}

impl<'a> State<'a> {
    fn new(syntax: &'a Syntax<'a>) -> State<'a> {
        State {
            syntax,
            loop_depth: Cell::new(0),
        }
    }

    fn enter_loop(&self) {
        self.loop_depth.set(self.loop_depth.get() + 1);
    }

    fn leave_loop(&self) {
        self.loop_depth.set(self.loop_depth.get() - 1);
    }

    fn is_in_loop(&self) -> bool {
        self.loop_depth.get() > 0
    }
}

impl From<char> for Whitespace {
    fn from(c: char) -> Self {
        match c {
            '+' => Self::Preserve,
            '-' => Self::Suppress,
            '~' => Self::Minimize,
            _ => panic!("unsupported `Whitespace` conversion"),
        }
    }
}

/// Parse template source to an abstract syntax tree.
///
/// Tries to parse the provided template string using the given syntax.
pub(crate) fn parse<'a>(src: &'a str, syntax: &'a Syntax<'_>) -> Result<Block<'a>, ParseError> {
    let state = State::new(syntax);
    let mut p = all_consuming(complete(|i| Node::parse(i, &state)));
    match p(src) {
        Ok((_, nodes)) => {
            let ws = Ws::default();
            Ok(Block { nodes, ws })
        }

        Err(nom::Err::Error(err)) | Err(nom::Err::Failure(err)) => {
            let nom::error::Error { input, .. } = err;
            let offset = src.len() - input.len();
            let (source_before, source_after) = src.split_at(offset);

            let snippet = match source_after.char_indices().enumerate().take(41).last() {
                Some((40, (i, _))) => format!("{:?}...", &source_after[..i]),
                _ => format!("{source_after:?}"),
            };

            let (row, column) = match source_before.lines().enumerate().last() {
                Some((row, last_line)) => (row + 1, last_line.chars().count()),
                None => (1, 0),
            };

            Err(ParseError {
                row,
                column,
                snippet,
            })
        }

        Err(nom::Err::Incomplete(_)) => unreachable!(),
    }
}

/// An error encountered when parsing template source.
#[derive(Debug)]
pub(crate) struct ParseError {
    row: usize,
    column: usize,
    snippet: String,
}

#[cfg(test)]
impl ParseError {
    /// The line number in the source where the error was identified.
    pub(crate) fn line(&self) -> usize {
        self.row
    }

    /// The column number in the source where the error was identified.
    pub(crate) fn column(&self) -> usize {
        self.column
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "problems parsing template source at row {}, column {} near:\n{}",
            self.row, self.column, self.snippet,
        )
    }
}

impl std::error::Error for ParseError {}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn not_ws(c: char) -> bool {
    !is_ws(c)
}

fn ws<'a, O>(
    inner: impl FnMut(&'a str) -> IResult<&'a str, O>,
) -> impl FnMut(&'a str) -> IResult<&'a str, O> {
    delimited(take_till(not_ws), inner, take_till(not_ws))
}

fn split_ws_parts(s: &str) -> Lit<'_> {
    let trimmed_start = s.trim_start_matches(is_ws);
    let len_start = s.len() - trimmed_start.len();
    let val = trimmed_start.trim_end_matches(is_ws);
    let lws = &s[..len_start];
    let rws = &trimmed_start[val.len()..];
    Lit { lws, val, rws }
}

/// Skips input until `end` was found, but does not consume it.
/// Returns tuple that would be returned when parsing `end`.
fn skip_till<'a, O>(
    end: impl FnMut(&'a str) -> IResult<&'a str, O>,
) -> impl FnMut(&'a str) -> IResult<&'a str, (&'a str, O)> {
    enum Next<O> {
        IsEnd(O),
        NotEnd(char),
    }
    let mut next = alt((map(end, Next::IsEnd), map(anychar, Next::NotEnd)));
    move |start: &'a str| {
        let mut i = start;
        loop {
            let (j, is_end) = next(i)?;
            match is_end {
                Next::IsEnd(lookahead) => return Ok((i, (j, lookahead))),
                Next::NotEnd(_) => i = j,
            }
        }
    }
}

fn keyword<'a>(k: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |i: &'a str| -> IResult<&'a str, &'a str> {
        let (j, v) = identifier(i)?;
        if k == v {
            Ok((j, v))
        } else {
            Err(nom::Err::Error(error_position!(i, ErrorKind::Tag)))
        }
    }
}

fn identifier(input: &str) -> IResult<&str, &str> {
    recognize(pair(identifier_start, opt(identifier_tail)))(input)
}

fn identifier_start(s: &str) -> IResult<&str, &str> {
    s.split_at_position1_complete(
        |c| !(c.is_alpha() || c == '_' || c >= '\u{0080}'),
        nom::error::ErrorKind::Alpha,
    )
}

fn identifier_tail(s: &str) -> IResult<&str, &str> {
    s.split_at_position1_complete(
        |c| !(c.is_alphanum() || c == '_' || c >= '\u{0080}'),
        nom::error::ErrorKind::Alpha,
    )
}

fn bool_lit(i: &str) -> IResult<&str, &str> {
    alt((keyword("false"), keyword("true")))(i)
}

fn num_lit(i: &str) -> IResult<&str, &str> {
    recognize(pair(digit1, opt(pair(char('.'), digit1))))(i)
}

fn str_lit(i: &str) -> IResult<&str, &str> {
    let (i, s) = delimited(
        char('"'),
        opt(escaped(is_not("\\\""), '\\', anychar)),
        char('"'),
    )(i)?;
    Ok((i, s.unwrap_or_default()))
}

fn char_lit(i: &str) -> IResult<&str, &str> {
    let (i, s) = delimited(
        char('\''),
        opt(escaped(is_not("\\\'"), '\\', anychar)),
        char('\''),
    )(i)?;
    Ok((i, s.unwrap_or_default()))
}

fn nested_parenthesis(i: &str) -> IResult<&str, ()> {
    let mut nested = 0;
    let mut last = 0;
    let mut in_str = false;
    let mut escaped = false;

    for (i, b) in i.chars().enumerate() {
        if !(b == '(' || b == ')') || !in_str {
            match b {
                '(' => nested += 1,
                ')' => {
                    if nested == 0 {
                        last = i;
                        break;
                    }
                    nested -= 1;
                }
                '"' => {
                    if in_str {
                        if !escaped {
                            in_str = false;
                        }
                    } else {
                        in_str = true;
                    }
                }
                '\\' => {
                    escaped = !escaped;
                }
                _ => (),
            }
        }

        if escaped && b != '\\' {
            escaped = false;
        }
    }

    if nested == 0 {
        Ok((&i[last..], ()))
    } else {
        Err(nom::Err::Error(error_position!(
            i,
            ErrorKind::SeparatedNonEmptyList
        )))
    }
}

fn path(i: &str) -> IResult<&str, Vec<&str>> {
    let root = opt(value("", ws(tag("::"))));
    let tail = separated_list1(ws(tag("::")), identifier);

    match tuple((root, identifier, ws(tag("::")), tail))(i) {
        Ok((i, (root, start, _, rest))) => {
            let mut path = Vec::new();
            path.extend(root);
            path.push(start);
            path.extend(rest);
            Ok((i, path))
        }
        Err(err) => {
            if let Ok((i, name)) = identifier(i) {
                // The returned identifier can be assumed to be path if:
                // - Contains both a lowercase and uppercase character, i.e. a type name like `None`
                // - Doesn't contain any lowercase characters, i.e. it's a constant
                // In short, if it contains any uppercase characters it's a path.
                if name.contains(char::is_uppercase) {
                    return Ok((i, vec![name]));
                }
            }

            // If `identifier()` fails then just return the original error
            Err(err)
        }
    }
}

fn take_content<'a>(i: &'a str, s: &State<'_>) -> IResult<&'a str, Node<'a>> {
    let p_start = alt((
        tag(s.syntax.block_start),
        tag(s.syntax.comment_start),
        tag(s.syntax.expr_start),
    ));

    let (i, _) = not(eof)(i)?;
    let (i, content) = opt(recognize(skip_till(p_start)))(i)?;
    let (i, content) = match content {
        Some("") => {
            // {block,comment,expr}_start follows immediately.
            return Err(nom::Err::Error(error_position!(i, ErrorKind::TakeUntil)));
        }
        Some(content) => (i, content),
        None => ("", i), // there is no {block,comment,expr}_start: take everything
    };
    Ok((i, Node::Lit(split_ws_parts(content))))
}

fn tag_block_start<'a>(i: &'a str, s: &State<'_>) -> IResult<&'a str, &'a str> {
    tag(s.syntax.block_start)(i)
}

fn tag_block_end<'a>(i: &'a str, s: &State<'_>) -> IResult<&'a str, &'a str> {
    tag(s.syntax.block_end)(i)
}

fn tag_comment_start<'a>(i: &'a str, s: &State<'_>) -> IResult<&'a str, &'a str> {
    tag(s.syntax.comment_start)(i)
}

fn tag_comment_end<'a>(i: &'a str, s: &State<'_>) -> IResult<&'a str, &'a str> {
    tag(s.syntax.comment_end)(i)
}

fn tag_expr_start<'a>(i: &'a str, s: &State<'_>) -> IResult<&'a str, &'a str> {
    tag(s.syntax.expr_start)(i)
}

fn tag_expr_end<'a>(i: &'a str, s: &State<'_>) -> IResult<&'a str, &'a str> {
    tag(s.syntax.expr_end)(i)
}
