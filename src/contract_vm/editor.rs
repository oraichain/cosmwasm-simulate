use std::borrow::Cow::{self, Borrowed, Owned};

use colored::*;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::validate::{self, MatchingBracketValidator, Validator};
use rustyline::{CompletionType, Config, Context, Editor};
use rustyline_derive::{Completer, Helper};

#[derive(Completer, Helper)]
pub struct MyHelper {
    highlighter: MatchingBracketHighlighter,
    validator: MatchingBracketValidator,
    hinter: HistoryHinter,
    colored_prompt: String,
}

impl Hinter for MyHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Highlighter for MyHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default {
            Borrowed(&self.colored_prompt)
        } else {
            Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned(format!("{}", hint.green().bold()))
    }

    fn highlight<'l>(&self, line: &'l str, pos: usize) -> Cow<'l, str> {
        self.highlighter.highlight(line, pos)
    }

    fn highlight_char(&self, line: &str, pos: usize) -> bool {
        self.highlighter.highlight_char(line, pos)
    }
}

impl Validator for MyHelper {
    fn validate(
        &self,
        ctx: &mut validate::ValidationContext,
    ) -> rustyline::Result<validate::ValidationResult> {
        self.validator.validate(ctx)
    }

    fn validate_while_typing(&self) -> bool {
        self.validator.validate_while_typing()
    }
}

pub struct TerminalEditor {
    rl: Editor<MyHelper>,
    history_entries: Vec<String>,
}

impl TerminalEditor {
    pub fn new() -> Self {
        let config = Config::builder()
            .history_ignore_space(true)
            .completion_type(CompletionType::List)
            .build();

        let h = MyHelper {
            highlighter: MatchingBracketHighlighter::new(),
            hinter: HistoryHinter {},
            colored_prompt: "".to_owned(),
            validator: MatchingBracketValidator::new(),
        };
        let mut rl = Editor::with_config(config);
        rl.set_helper(Some(h));

        TerminalEditor {
            rl,
            history_entries: vec![],
        }
    }

    pub fn clear_history(&mut self) {
        self.rl.clear_history()
    }

    pub fn add_history_entry(&mut self, line: &str) -> bool {
        self.rl.add_history_entry(line)
    }

    /// this is permanent
    pub fn add_input_history_entry(&mut self, line: String) {
        self.history_entries.push(line)
    }

    pub fn update_input_history_entry(&mut self) -> bool {
        self.update_history_entries(self.history_entries.clone())
    }

    pub fn update_history_entries(&mut self, lines: Vec<String>) -> bool {
        self.rl.clear_history();
        for line in lines {
            if !self.rl.add_history_entry(line) {
                return false;
            }
        }
        return true;
    }

    pub fn readline(&mut self, input_data: &mut String, store_input: bool) -> bool {
        let p = ">> ";
        self.rl.helper_mut().expect("No helper").colored_prompt = format!("{}", p.green().bold());
        let readline = self.rl.readline(&p);

        match readline {
            Ok(line) => {
                let data = line.trim();
                input_data.push_str(data);
                if store_input {
                    self.add_input_history_entry(data.to_string());
                }
            }

            // Ctrl + C to break
            Err(rustyline::error::ReadlineError::Interrupted) => {
                std::process::exit(0);
            }

            Err(error) => {
                println!("error: {}", error);
                return false;
            }
        }

        return true;
    }
}
