use std::borrow::Cow::{self, Borrowed, Owned};

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::config::OutputStreamType;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::validate::{self, MatchingBracketValidator, Validator};
use rustyline::{CompletionType, Config, Context, EditMode, Editor};
use rustyline_derive::Helper;

#[derive(Helper)]
pub struct MyHelper {
    completer: FilenameCompleter,
    highlighter: MatchingBracketHighlighter,
    validator: MatchingBracketValidator,
    hinter: HistoryHinter,
    colored_prompt: String,
}

impl Completer for MyHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        self.completer.complete(line, pos, ctx)
    }
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
        Owned("\x1b[1m\x1b[32m".to_owned() + hint + "\x1b[0m")
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
            .edit_mode(EditMode::Emacs)
            .output_stream(OutputStreamType::Stdout)
            .build();
        let h = MyHelper {
            completer: FilenameCompleter::new(),
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
        self.rl.helper_mut().expect("No helper").colored_prompt = format!("\x1b[1;32m{}\x1b[0m", p);
        let readline = self.rl.readline(&p);

        match readline {
            Ok(line) => {
                let data = line.trim();
                input_data.push_str(data);
                if store_input {
                    self.add_input_history_entry(data.to_string());
                }
                self.rl.add_history_entry(data);
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
