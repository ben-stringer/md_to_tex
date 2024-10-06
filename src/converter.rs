use anyhow::{anyhow, bail, Error};
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use smallvec::{smallvec, SmallVec};
use std::io::{self, BufRead};

// Constant values; must be loaded lazily because they can panic (only if the regex is bad)
lazy_static! {
    static ref RE_LINK_TO_LOCAL: Regex =
        Regex::new(r#"^\[(?<label>.+)]\(\./(?<path>.+).md\)$"#).unwrap();
    static ref RE_CHAPTER_HEADER: Regex =
        Regex::new(r#"^## (\[]\{#(?<label>.+)\})?(?<head>.*)$"#).unwrap();
    static ref RE_SECTION_HEADER: Regex =
        Regex::new(r#"^### (\[]\{#(?<label>.+)\})?(?<head>.*)$"#).unwrap();
    static ref RE_SUBSECTION_HEADER: Regex =
        Regex::new(r#"^#### (\[]\{#(?<label>.+)\})?(?<head>.*)$"#).unwrap();
    static ref RE_SUBSUBSECTION_HEADER: Regex =
        Regex::new(r#"^##### (\[]\{#(?<label>.+)\})?(?<head>.*)$"#).unwrap();
    static ref RE_TABLE_HEADER: Regex = Regex::new(r#"(<!--(?<desc>.+)-->)?(?<label>.*)"#).unwrap();
    static ref RE_START_ENUMERATE: Regex = Regex::new(r#"^[0-9]+\. (?<item>.+)$"#).unwrap();
    static ref RE_START_ITEMIZE: Regex = Regex::new(r#"^[*+-] (?<item>.+)$"#).unwrap();
    static ref RE_LINK: Regex = Regex::new(r#"\[(?<text>.+)]\((?<link>.+)\)"#).unwrap();
    static ref RE_SUPERSCRIPT: Regex = Regex::new(r#"\^(?<super>.+?)\^"#).unwrap();
    static ref RE_BOLD_FONT: Regex = Regex::new(r#"\*(?<bold>.+?)\*"#).unwrap();
    static ref RE_MONO_FONT: Regex = Regex::new(r#"`(?<mono>.+?)`"#).unwrap();
    static ref RE_SINGLE_QUOTE: Regex = Regex::new(r#"'(?<quote>.+?)'"#).unwrap();
    static ref RE_DOUBLE_QUOTE: Regex = Regex::new(r#""(?<quote>.+?)""#).unwrap();
    static ref RE_EMPH_FONT: Regex = Regex::new(r#"_(?<emph>.+?)_"#).unwrap();
    static ref RE_FOOTNOTE_REF: Regex = Regex::new(r#"\[\^(?<mark>.+?)]"#).unwrap();
    static ref RE_FOOTNOTE_BODY: Regex = Regex::new(r#"^\[\^(?<mark>.+?)](?<body>.+?)$"#).unwrap();
    static ref RE_COMMENT: Regex = Regex::new(r#"<!--(.*)-->"#).unwrap();
    static ref RE_LINE_COMMENT: Regex = Regex::new(r#"^<!--(.*)-->$"#).unwrap();
    static ref RE_NUM_EQUATION: Regex = Regex::new(r#"^\$\$<!--(?<label>.+)-->$"#).unwrap();
    static ref RE_CODE_HERE: Regex = Regex::new(r#"```(?<lang>.+)"#).unwrap();
    static ref RE_CODE_FLOAT: Regex =
        Regex::new(r#"```(?<lang>.+)<!--(?<label>.+)--><!--(?<caption>.+)-->"#).unwrap();
}
/// Main entry point of the md processor.
/// Note that this function does not actually process a single line of text.
/// Instead, it returns an iterator.
/// It is the caller's responsibility to consume the iterator,
/// doing something with the transformed data, e.g., print to std out or write to a file.
/// This function consumes the supplied value.
/// Errors are printed to stderr.  A future version may return an iterator over Result objects.
pub fn convert(lines: io::Lines<impl BufRead>) -> impl Iterator<Item = String> {
    let mut state: State = State::Text;

    lines
        .map(move |res_line| {
            res_line
                .map_err(|err| anyhow!(err))
                .and_then(|line| state.process_line(&line))
                .inspect_err(|err| eprintln!("{}", err))
                .ok()
                .map(|(new_state, processed_line)| {
                    state = new_state;
                    processed_line
                })
        })
        .filter(Option::is_some)
        .map(Option::unwrap)
}

/// Processing is modeled on a state machine.
/// These are the states that we could be in.
#[derive(PartialEq)]
enum State {
    Ordered(SmallVec<[u8; 4]>),
    Unordered(SmallVec<[u8; 4]>),
    Quote,
    Code,
    Figure,
    FigureCaption,
    TableHeader,
    TableBody(bool),
    TableCaption,
    Literal,
    Text,
    FootnoteBody,
    NumberedEquation,
    UnnumberedEquation,
}

impl State {
    /// State has one function, process the line.
    /// This function determines which state we are currently in and calls the
    /// appropriate function.  It's like dynamic dispatch, except not.
    fn process_line(&self, line: &str) -> Result<(State, String), Error> {
        match self {
            State::Ordered(indents) => process_line_ordered(line, indents),
            State::Unordered(indents) => process_line_unordered(line, indents),
            State::Quote => process_line_quote(line),
            State::Code => process_line_code(line),
            State::Figure => process_line_figure(line),
            State::FigureCaption => process_line_figure_caption(line),
            State::TableHeader => process_line_table_header(line),
            State::TableBody(line_every_row) => process_line_table_body(line, *line_every_row),
            State::TableCaption => process_line_table_caption(line),
            State::Literal => process_literal(line),
            State::FootnoteBody => process_footnote_body(line),
            State::Text => process_line_text(line),
            State::UnnumberedEquation => process_unnumbered_equation_text(line),
            State::NumberedEquation => process_numbered_equation_text(line),
        }
    }
}

/// Process a simple string.
/// We are not concerned with sections, tables, lists, etc here.
/// This is just a plain old piece of text, maybe in the document body,
/// maybe in a figure caption, maybe in a table row.
/// It may have bold text, italics, superscripts, and so on.
/// It may have single or double quotes.
/// This translation to tex happens here.
fn simple_string_process(line: &str) -> String {
    let mut res = line.to_owned();
    res = res.replace('&', "\\&");
    res = RE_COMMENT.replace_all(&res, String::new()).to_string();
    res = RE_SUPERSCRIPT
        .replace_all(&res, |cap: &Captures| {
            format!(r"\textsuperscript{{{}}}", &cap["super"])
        })
        .to_string();
    res = RE_BOLD_FONT
        .replace_all(&res, |cap: &Captures| {
            format!(r"\textbf{{{}}}", &cap["bold"])
        })
        .to_string();
    res = RE_MONO_FONT
        .replace_all(&res, |cap: &Captures| {
            format!(r"\texttt{{{}}}", &cap["mono"])
        })
        .to_string();
    res = RE_SINGLE_QUOTE
        .replace_all(&res, |cap: &Captures| format!("`{}'", &cap["quote"]))
        .to_string();
    res = RE_DOUBLE_QUOTE
        .replace_all(&res, |cap: &Captures| format!("``{}''", &cap["quote"]))
        .to_string();
    res = RE_EMPH_FONT
        .replace_all(&res, |cap: &Captures| format!(r"\emph{{{}}}", &cap["emph"]))
        .to_string();
    res = RE_LINK
        .replace_all(&res, |cap: &Captures| {
            format!(r"{} \url{{{}}}", &cap["text"], &cap["link"])
        })
        .to_string();
    res = RE_FOOTNOTE_REF
        .replace_all(&res, |cap: &Captures| {
            format!(r"\footnotemark[{}]", &cap["mark"])
        })
        .to_string();

    res
}

fn process_line_ordered(line: &str, indents: &SmallVec<[u8; 4]>) -> Result<(State, String), Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        // Close out all open itemizes
        Ok((
            State::Text,
            indents.iter().map(|_| "\\end{enumerate}").join("\n") + "\n",
        ))
    } else {
        if let Some(cap) = RE_START_ENUMERATE.captures(trimmed) {
            // Line starts with a '* ' or '+ ' or '- ', which is an itemized list.

            let indent_u = line.chars().take_while(|ch| ch.is_whitespace()).count();
            if indent_u > u8::max_value() as usize {
                bail!(
                    "Leading indent cannot be more than {}, however I got {}.",
                    u8::max_value(),
                    indent_u
                );
            }
            let indent = indent_u as u8;
            let prev_indent = indents
                .last()
                .expect("This function shouldn't be called with an empty indents vec.");

            if &indent == prev_indent {
                // indent hasn't changed
                let mut item = r#"\item "#.to_owned();
                item.push_str(&simple_string_process(&cap["item"]));
                item.push('\n');
                Ok((State::Ordered(indents.to_owned()), item))
            } else if &indent > prev_indent {
                // indent increased
                if indents.len() == indents.capacity() {
                    bail!("Exceeded this tool's hard-coded limit on the level of nesting of enumerate components.");
                }
                let mut sub_list = "\\begin{enumerate}\n".to_owned();
                sub_list.push_str("\\item ");
                sub_list.push_str(&simple_string_process(&cap["item"]));
                sub_list.push('\n');
                let next_indents = {
                    let mut tmp = indents.to_owned();
                    tmp.push(indent);
                    tmp
                };
                Ok((State::Ordered(next_indents), sub_list))
            } else
            /* if indent < current_index */
            {
                // indent decreased
                // close out the current list and then recursively call this function.
                // Why recursion?
                // Imagine our indents are [2, 4] and the current indent is 3.
                // We close this one at 4 but then start a new one.
                if indents.len() <= 1 {
                    // We may close several open enumerates, however if we end up with something like
                    // indents == [4, 8] and the current indent is 2, this is an error.
                    bail!("Indent level cannot be smaller than the initial indent");
                }
                let list_close = "\\end{enumerate}\n".to_owned();
                let next_indents = {
                    let mut tmp = indents.to_owned();
                    tmp.pop();
                    tmp
                };
                let subprocessing = process_line_ordered(line, &next_indents)?;
                Ok((subprocessing.0, list_close + &subprocessing.1))
            }
        } else {
            // Continuation of the current item
            Ok((
                State::Ordered(indents.to_owned()),
                simple_string_process(trimmed) + "\n",
            ))
        }
    }
}
fn process_line_unordered(
    line: &str,
    indents: &SmallVec<[u8; 4]>,
) -> Result<(State, String), Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        // Close out all open itemizes
        Ok((
            State::Text,
            indents.iter().map(|_| "\\end{itemize}").join("\n") + "\n",
        ))
    } else {
        if let Some(cap) = RE_START_ITEMIZE.captures(trimmed) {
            // Line starts with a '* ' or '+ ' or '- ', which is an itemized list.

            let indent_u = line.chars().take_while(|ch| ch.is_whitespace()).count();
            if indent_u > u8::max_value() as usize {
                bail!(
                    "Leading indent cannot be more than {}, however I got {}.",
                    u8::max_value(),
                    indent_u
                );
            }
            let indent = indent_u as u8;
            let prev_indent = indents
                .last()
                .expect("This function shouldn't be called with an empty indents vec.");

            if &indent == prev_indent {
                // indent hasn't changed
                let mut item = r#"\item "#.to_owned();
                item.push_str(&simple_string_process(&cap["item"]));
                item.push('\n');
                Ok((State::Unordered(indents.to_owned()), item))
            } else if &indent > prev_indent {
                // indent increased
                if indents.len() == indents.capacity() {
                    bail!("Exceeded this tool's hard-coded limit on the level of nesting of itemize components.");
                }
                let mut sub_list = "\\begin{itemize}\n".to_owned();
                sub_list.push_str("\\item ");
                sub_list.push_str(&simple_string_process(&cap["item"]));
                sub_list.push('\n');
                let next_indents = {
                    let mut tmp = indents.to_owned();
                    tmp.push(indent);
                    tmp
                };
                Ok((State::Unordered(next_indents), sub_list))
            } else
            /* if indent < current_index */
            {
                // indent decreased
                // close out the current list and then recursively call this function.
                // Why recursion?
                // Imagine our indents are [2, 4] and the current indent is 3.
                // We close this one at 4 but then start a new one.
                if indents.len() <= 1 {
                    // We may close several open itemizes, however if we end up with something like
                    // indents == [4, 8] and the current indent is 2, this is an error.
                    bail!("Indent level cannot be smaller than the initial indent");
                }
                let list_close = "\\end{itemize}\n".to_owned();
                let next_indents = {
                    let mut tmp = indents.to_owned();
                    tmp.pop();
                    tmp
                };
                let subprocessing = process_line_unordered(line, &next_indents)?;
                Ok((subprocessing.0, list_close + &subprocessing.1))
            }
        } else {
            // Continuation of the current item
            Ok((
                State::Unordered(indents.to_owned()),
                simple_string_process(trimmed) + "\n",
            ))
        }
    }
}
fn process_line_quote(line: &str) -> Result<(State, String), Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok((State::Text, "\\end{displayquote}\n\n".to_owned()))
    } else {
        if trimmed.starts_with("> ") {
            Ok((State::Quote, simple_string_process(&trimmed[2..]) + "\n"))
        } else if trimmed.starts_with(">") {
            Ok((State::Quote, simple_string_process(&trimmed[1..]) + "\n"))
        } else {
            Ok((State::Quote, simple_string_process(trimmed) + "\n"))
        }
    }
}
fn process_line_code(line: &str) -> Result<(State, String), Error> {
    if line == "```" {
        Ok((State::Text, "\\end{lstlisting}\n".to_owned()))
    } else {
        // Don't do simple processing here because this is verbatim code.
        // We do need to add a newline though
        let code = format!("{}\n", line);
        Ok((State::Code, code))
    }
}
fn process_line_figure(line: &str) -> Result<(State, String), Error> {
    if line.trim().is_empty() {
        Ok((State::FigureCaption, "\n\\caption{".to_owned()))
    } else {
        // Don't do simple processing here because this is likely already tex
        // We do need to add a newline though
        let fig = format!("{}\n", line);
        Ok((State::Figure, fig))
    }
}
fn process_line_figure_caption(line: &str) -> Result<(State, String), Error> {
    if line.trim().is_empty() {
        Ok((State::Text, "}\n\\end{figure}\n\n".to_owned()))
    } else {
        let caption = format!(
            "{}\n",
            if line.trim().starts_with("\\label{") {
                line.to_owned()
            } else {
                simple_string_process(line)
            }
        );
        Ok((State::FigureCaption, caption))
    }
}
fn process_line_table_header(line: &str) -> Result<(State, String), Error> {
    let trimmed = line.trim();
    if trimmed.starts_with("|---") || trimmed.starts_with("| ---") {
        // The table's header line; just skip it
        Ok((State::TableHeader, String::new()))
    } else if trimmed.contains("line every row") {
        Ok((State::TableBody(true), String::new()))
    } else {
        if trimmed.contains("line header only") {
            Ok((State::TableBody(false), "\\midrule\n".to_owned()))
        } else {
            Ok((State::TableBody(false), String::new()))
        }
    }
}

fn process_line_table_body(line: &str, line_every_row: bool) -> Result<(State, String), Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok((
            State::TableCaption,
            "\\bottomrule\n\\end{tabular}\n\\caption{".to_owned(),
        ))
    } else {
        let mut body = if line_every_row { "\\midrule\n" } else { "" }.to_owned();
        body.push_str(
            trimmed[1..]
                .split_terminator('|')
                .map(str::trim)
                .map(simple_string_process)
                .collect::<Vec<_>>()
                .join(" & ")
                .as_str(),
        );
        body.push_str(" \\\\\n");
        Ok((State::TableBody(line_every_row), body))
    }
}
fn process_line_table_caption(line: &str) -> Result<(State, String), Error> {
    if line.is_empty() {
        Ok((State::Text, "}\n\\end{table}\n\n".to_owned()))
    } else {
        let caption = format!(
            "{}\n",
            if line.trim().starts_with("\\label{") {
                line.to_owned()
            } else {
                simple_string_process(line)
            }
        );
        Ok((State::TableCaption, caption))
    }
}
fn process_literal(line: &str) -> Result<(State, String), Error> {
    if line.is_empty() {
        Ok((State::Text, "".to_owned()))
    } else {
        Ok((State::Literal, format!("{}\n", line)))
    }
}
fn process_footnote_body(line: &str) -> Result<(State, String), Error> {
    if line.is_empty() {
        Ok((State::Text, "}\n\n".to_owned()))
    } else {
        Ok((State::FootnoteBody, simple_string_process(line)))
    }
}
fn process_unnumbered_equation_text(line: &str) -> Result<(State, String), Error> {
    if line == "$$" {
        Ok((State::Text, "\\end{equation*}".to_owned()))
    } else {
        Ok((State::UnnumberedEquation, line.to_owned()))
    }
}
fn process_numbered_equation_text(line: &str) -> Result<(State, String), Error> {
    if line == "$$" {
        Ok((State::Text, "\\end{equation}".to_owned()))
    } else {
        Ok((State::NumberedEquation, line.to_owned()))
    }
}
fn process_line_text(line: &str) -> Result<(State, String), Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        // A new paragraph
        Ok((State::Text, "\n".to_owned()))
    } else if trimmed.starts_with("# ") {
        // Line is a top-level heading; treat it as a comment
        // There should only be one top-level heading per markdown anyway
        Ok((State::Text, String::new()))
    } else if let Some(cap) = RE_LINK_TO_LOCAL.captures(trimmed) {
        let path = cap
            .name("path")
            .expect("Should not fail to get a path if the regex captures");
        Ok((State::Text, format!("\\input{{{}}}\n", path.as_str())))
    } else if let Some(cap) = RE_SUBSUBSECTION_HEADER.captures(trimmed) {
        let mut text = format!("\\subsubsection{{{}}}", &cap["head"]);
        if let Some(l) = cap.name("label").map(|m| m.as_str()) {
            text.push_str(&format!("\\label{{{}}}", l));
        }
        text.push('\n');
        Ok((State::Text, text))
    } else if let Some(cap) = RE_SUBSECTION_HEADER.captures(trimmed) {
        let mut text = format!("\\subsection{{{}}}", &cap["head"]);
        if let Some(l) = cap.name("label").map(|m| m.as_str()) {
            text.push_str(&format!("\\label{{{}}}", l));
        }
        text.push('\n');
        Ok((State::Text, text))
    } else if let Some(cap) = RE_SECTION_HEADER.captures(trimmed) {
        let mut text = format!("\\section{{{}}}", &cap["head"]);
        if let Some(l) = cap.name("label").map(|m| m.as_str()) {
            text.push_str(&format!("\\label{{{}}}", l));
        }
        text.push('\n');
        Ok((State::Text, text))
    } else if let Some(cap) = RE_CHAPTER_HEADER.captures(trimmed) {
        let mut text = format!("\\chapter{{{}}}", &cap["head"]);
        if let Some(l) = cap.name("label").map(|m| m.as_str()) {
            text.push_str(&format!("\\label{{{}}}", l));
        }
        text.push('\n');
        Ok((State::Text, text))
    } else if trimmed == "|figure" {
        Ok((State::Figure, "\\begin{figure}\n".to_owned()))
    } else if trimmed == "|literal" {
        Ok((State::Literal, "".to_owned()))
    } else if trimmed.starts_with('|') {
        // Test for table must follow test for figure since both start with a pipe
        if !trimmed.ends_with('|') {
            // It's easier to barf than handle this case right now
            bail!("Unexpected line ending for table.  The line starts with '|' but does not end with '|'.\n{}", line);
        }
        // The heading text and formatting strings are in the same line in markdown
        let column_re_captures = trimmed[1..]
            .split_terminator('|')
            .map(str::trim)
            .map(|h| RE_TABLE_HEADER.captures(h))
            .collect::<Vec<_>>();
        // If any columns don't have contents, we should fail
        if 0 < column_re_captures
            .iter()
            .filter(|col| col.is_none())
            .count()
        {
            bail!("Unable to process table headings for string {}", line);
        }
        let columns = column_re_captures
            .iter()
            .map(|opt| opt.as_ref().unwrap())
            .collect::<Vec<_>>();

        let mut table = String::new();
        table.push_str("\\begin{table}\n\\begin{tabular}{");
        table.push_str(
            columns
                .iter()
                // If no column format is specified, default to centered
                .map(|cap| cap.name("desc").map_or("c", |m| m.as_str().trim()))
                .collect::<Vec<_>>()
                .join(" ")
                .as_str(),
        );
        table.push_str("}\n\\toprule\n");
        table.push_str(
            columns
                .iter()
                // Should really bail if I can't pull a column label, but for now,
                // just inserting the word ERROR
                .map(|cap| cap.name("label").map_or("ERROR", |m| m.as_str().trim()))
                .map(|label| format!("\\textbf{{{}}}", label))
                .collect::<Vec<_>>()
                .join(" & ")
                .as_str(),
        );
        table.push_str(" \\\\\n");
        Ok((State::TableHeader, table))
    } else if let Some(cap) = RE_CODE_FLOAT.captures(trimmed) {
        let mut listing = "\\begin{lstlisting}".to_owned();
        let lang = cap.name("lang").map_or("ERROR", |m| m.as_str().trim());
        let label = cap.name("label").map_or("ERROR", |m| m.as_str().trim());
        let caption = cap.name("caption").map_or("ERROR", |m| m.as_str().trim());
        listing.push_str(&format!(
            "[\n\tstyle={},\n\tlanguage={},\n\tlabel={},\n\tcaption={{{}}},\n\tfloat]",
            lang, lang, label, caption
        ));
        listing.push('\n');
        Ok((State::Code, listing))
    } else if let Some(cap) = RE_CODE_HERE.captures(trimmed) {
        let lang = cap.name("lang").map_or("ERROR", |m| m.as_str().trim());
        let mut listing = "\\begin{lstlisting}".to_owned();
        if !lang.is_empty() {
            listing.push_str(&format!("[style={},language={}]", lang, lang));
        }
        listing.push('\n');
        Ok((State::Code, listing))
    } else if trimmed.starts_with("> ") {
        // Start of a quote environment
        // TODO: What about quotes that start with four spaces?
        // We've already trimmed off the leading spaces!
        // For now, we don't support that method for quoting
        let mut quote = "\\begin{displayquote}\n".to_owned();
        quote.push_str(&simple_string_process(&line[2..]));
        quote.push('\n');
        Ok((State::Quote, quote))
    } else if let Some(cap) = RE_START_ITEMIZE.captures(trimmed) {
        // Line starts with a '* ' or '+ ' or '- ', which is an itemized list.
        let mut list = "\\begin{itemize}\n".to_owned();
        list.push_str("\\item ");
        list.push_str(&simple_string_process(&cap["item"]));
        list.push('\n');
        let indent = line.chars().take_while(|ch| ch.is_whitespace()).count();
        if indent > u8::max_value() as usize {
            Err(anyhow!(
                "Leading indent cannot be more than {}, however I got {}.",
                u8::max_value(),
                indent
            ))
        } else {
            Ok((State::Unordered(smallvec![indent as u8]), list))
        }
    } else if let Some(cap) = RE_START_ENUMERATE.captures(trimmed) {
        // Line starts with a number and a period.  This is an enumerated list
        let mut list = "\\begin{enumerate}\n".to_owned();
        list.push_str("\\item ");
        list.push_str(&simple_string_process(&cap["item"]));
        list.push('\n');
        let indent = line.chars().take_while(|ch| ch.is_whitespace()).count();
        if indent > u8::max_value() as usize {
            Err(anyhow!(
                "Leading indent cannot be more than {}, however I got {}.",
                u8::max_value(),
                indent
            ))
        } else {
            Ok((State::Ordered(smallvec![indent as u8]), list))
        }
    } else if let Some(cap) = RE_FOOTNOTE_BODY.captures(trimmed) {
        let mut body = "\\footnotetext[".to_owned();
        body.push_str(&cap["mark"]);
        body.push_str("]{\n");
        body.push_str(&simple_string_process(&cap["body"]));
        body.push_str("\n");
        Ok((State::FootnoteBody, body))
    } else if trimmed == "$$" {
        Ok((State::UnnumberedEquation, "\\begin{equation*}\n".to_owned()))
    } else if let Some(cap) = RE_NUM_EQUATION.captures(trimmed) {
        let mut body = "\\begin{equation}\\label{".to_owned();
        body.push_str(&cap["label"]);
        body.push_str("}\n");
        Ok((State::NumberedEquation, body))
    } else if let Some(_) = RE_LINE_COMMENT.captures(trimmed) {
        // If we have a line comment, and strip it out using simple string process,
        // we end up with a blank line in the latex, which signals a new paragraph.
        Ok((State::Text, String::new()))
    } else {
        // Nothing special about this line, just regular ol' simple markdown
        Ok((State::Text, format!("{}\n", simple_string_process(line))))
    }
}

#[cfg(test)]
mod re_tests {
    /// For testing the regular expressions
    use super::*;

    #[test]
    fn test_all() {
        let v: Vec<(&str, &Regex)> = vec![
            ("##", &RE_CHAPTER_HEADER),
            ("###", &RE_SECTION_HEADER),
            ("####", &RE_SUBSECTION_HEADER),
            ("#####", &RE_SUBSUBSECTION_HEADER),
        ];
        v.iter().for_each(|(prefix, re)| {
            test_header_simple(prefix, re);
            test_header_with_label(prefix, re);
        });
    }

    fn test_header_simple(prefix: &str, re: &Regex) {
        let expected_head = "The Chapter/Section/Sub... Header";
        let test_str = format!("{} {}", prefix, expected_head);

        let o_cap = re.captures(&test_str);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();
        let o_head = cap.name("head");
        assert!(o_head.is_some());
        let head = o_head.unwrap().as_str();
        assert_eq!(head, expected_head);
        let o_label = cap.name("label");
        assert!(o_label.is_none());
    }

    fn test_header_with_label(prefix: &str, re: &Regex) {
        let expected_head = "The Chapter/Section/Sub... Header";
        let expected_label = "lbl:rust:test";
        let test_str = format!("{} []{{#{}}}{}", prefix, expected_label, expected_head);

        let o_cap = re.captures(&test_str);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();
        let o_head = cap.name("head");
        assert!(o_head.is_some());
        let head = o_head.unwrap().as_str();
        assert_eq!(head, expected_head);
        let o_label = cap.name("label");
        assert!(o_label.is_some());
        let label = o_label.unwrap().as_str();
        assert_eq!(label, expected_label);
    }

    #[test]
    fn test_table_header_simple() {
        // r#"(<!--(?<desc>.+)-->)?(?<label>.*)"#
        // TODO: What if we want to support the label coming before the column specification?
        let expected_label = "Centered Column Header";
        let expected_desc = "c";
        let test_str = format!("<!-- {} --> {}", expected_desc, expected_label);

        let o_cap = RE_TABLE_HEADER.captures(&test_str);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();
        let o_label = cap.name("label");
        assert!(o_label.is_some());
        let head = o_label.unwrap().as_str().trim();
        assert_eq!(head, expected_label);
        let o_desc = cap.name("desc");
        assert!(o_desc.is_some());
        let desc = o_desc.unwrap().as_str().trim();
        assert_eq!(desc, expected_desc);
    }

    #[test]
    fn test_table_header_complex() {
        let expected_label = "Centered Column Header";
        let expected_desc = ">{\\raggedright\\arraybackslash}m{4cm}";
        let test_str = format!("  <!-- {}--> {}  ", expected_desc, expected_label);

        let o_cap = RE_TABLE_HEADER.captures(test_str.trim());
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();
        let o_label = cap.name("label");
        assert!(o_label.is_some());
        let label = o_label.unwrap().as_str().trim();
        assert_eq!(label, expected_label);
        let o_desc = cap.name("desc");
        assert!(o_desc.is_some());
        let desc = o_desc.unwrap().as_str().trim();
        assert_eq!(desc, expected_desc);
    }

    #[test]
    fn test_footnote_mark() {
        let footnote_mark = "asdf";
        let expected_text = format!(
            "This is a test\\footnotemark[{}] of the system.",
            footnote_mark
        );
        let test_str = format!("This is a test[^{}] of the system.", footnote_mark);

        let o_cap = RE_FOOTNOTE_REF.captures(&test_str);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();
        let o_mark = cap.name("mark");
        assert!(o_mark.is_some());
        let mark = o_mark.unwrap();
        assert!(mark.as_str() == footnote_mark);

        let processed = simple_string_process(&test_str);
        assert!(processed == expected_text);
    }

    #[test]
    fn test_footnote_text() {
        let footnote_mark = "asdf";
        let footnote_body = "This is a test of the system.";
        let expected_text = format!("\\footnotetext[{}]{{\n{}\n", footnote_mark, footnote_body);
        let test_str = format!("[^{}]{}", footnote_mark, footnote_body);

        let o_cap = RE_FOOTNOTE_BODY.captures(&test_str);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();

        let o_mark = cap.name("mark");
        assert!(o_mark.is_some());
        let mark = o_mark.unwrap();
        assert!(mark.as_str() == footnote_mark);

        let o_body = cap.name("body");
        assert!(o_body.is_some());
        let body = o_body.unwrap();
        assert!(body.as_str() == footnote_body);

        let r_processed = process_line_text(&test_str);
        assert!(r_processed.is_ok());
        let processed = r_processed.ok().unwrap();
        assert!(processed.0 == State::FootnoteBody);
        assert!(processed.1 == expected_text);
    }

    #[test]
    fn test_comments() {
        for test_str in [
            "<!-- This is a comment and is expected to be removed. -->\n",
            "   <!-- This is a comment and is expected to be removed. -->\n",
            "<!-- This is a comment and is expected to be removed. -->   \n",
            "   <!-- This is a comment and is expected to be removed. -->   \n",
            "\t<!-- This is a comment and is expected to be removed. -->\t\n",
            "  \t<!-- This is a comment and is expected to be removed. -->\t  \n",
            "\t  <!-- This is a comment and is expected to be removed. -->  \t\n",
            "  \t  <!-- This is a comment and is expected to be removed. -->  \t\n",
            "  \t  <!-- This is a comment and is expected to be removed. -->  \t  \n",
        ] {
            let processed = simple_string_process(&test_str);
            assert!(processed.trim().is_empty());
        }

        let comment_text = "<!-- This is a comment and is expected to be removed. -->";
        for (prefix, postfix) in [
            ("a", "b\n"),
            ("a   ", "b\n"),
            ("a", "   b\n"),
            ("a   ", "   b\n"),
            ("a\t", "\tb\n"),
            ("a  \t", "\t  b\n"),
            ("a\t  ", "  \tb\n"),
            ("a  \t  ", "  \tb\n"),
            ("a  \t  ", "  \t  b\n"),
        ] {
            let test_str = prefix.to_owned() + comment_text + postfix;
            let expected_str = prefix.to_owned() + postfix;
            let processed = simple_string_process(&test_str);
            assert!(processed == expected_str);
        }
    }

    #[test]
    fn test_page_inclusions() {
        // r#"^[(?<label>)]\(\./(?<path>.*)\)$"#
        let page_label = "This should be ignored";
        let raw_page_path = "a_linked/page";
        let md_page_path = format!("{}.md", raw_page_path);
        let page_link = format!("[{}](./{})", page_label, md_page_path);
        let expected_text = format!("\\input{{{}}}\n", raw_page_path);

        let o_cap = RE_LINK_TO_LOCAL.captures(&page_link);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();

        let o_label = cap.name("label");
        assert!(o_label.is_some());
        assert!(o_label.unwrap().as_str() == page_label);

        let o_path = cap.name("path");
        assert!(o_path.is_some());
        assert!(o_path.unwrap().as_str() == raw_page_path);

        let processed = process_line_text(&page_link);
        assert!(processed.is_ok());
        let (state, import) = processed.ok().unwrap();
        assert!(state == State::Text);
        assert!(import == expected_text);
    }

    #[test]
    fn test_equations() {
        let eqn_line = r#"$$<!--eq:test-->"#;
        let o_cap = RE_NUM_EQUATION.captures(&eqn_line);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();
        let o_label = cap.name("label");
        assert!(o_label.is_some());
        let label_text = o_label.unwrap();
        assert!(label_text.as_str() == "eq:test");
    }

    #[test]
    fn test_code_regex() {
        let code_line = r#"```python<!--lst:test--><!--Hello World, this is a caption!-->"#;
        let o_cap = RE_CODE_FLOAT.captures(&code_line);
        assert!(o_cap.is_some());
        let cap = o_cap.unwrap();

        let o_lang = cap.name("lang");
        assert!(o_lang.is_some());
        let lang_text = o_lang.unwrap();
        println!("{}", lang_text.as_str());
        assert!(lang_text.as_str() == "python");

        let o_label = cap.name("label");
        assert!(o_label.is_some());
        let label_text = o_label.unwrap();
        println!("{}", label_text.as_str());
        assert!(label_text.as_str() == "lst:test");

        let o_caption = cap.name("caption");
        assert!(o_caption.is_some());
        let caption_text = o_caption.unwrap();
        println!("{}", caption_text.as_str());
        assert!(caption_text.as_str() == "Hello World, this is a caption!");
    }
}
