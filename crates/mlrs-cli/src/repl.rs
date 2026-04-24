use std::cell::RefCell;
use std::sync::Arc;

use anyhow::Result;
use reedline::{
    Completer, Emacs, Highlighter, Hinter, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, Signal, Span, StyledText, Suggestion,
};
use slash_core::{
    command::{MethodDef, SlashCommand},
    env::SlenvLoader,
    executor::{CommandOutput, Execute, ExecutionError, Executor, PipeValue},
    registry::CommandRegistry,
};
use slash_lang::parser::ast::Arg;

use mlrs_core::Rlm;

// ---------------------------------------------------------------------------
// Shared REPL state (thread-local so slash commands can access it)
// ---------------------------------------------------------------------------

thread_local! {
    static REPL_STATE: RefCell<ReplState> = RefCell::new(ReplState::default());
}

#[derive(Default)]
struct ReplState {
    query: Option<String>,
    context: Option<String>,
}

// ---------------------------------------------------------------------------
// Slash commands
// ---------------------------------------------------------------------------

struct QueryCmd;
impl SlashCommand for QueryCmd {
    fn name(&self) -> &str {
        "query"
    }
    fn methods(&self) -> &[MethodDef] {
        &[]
    }
    fn execute(
        &self,
        primary: Option<&str>,
        _args: &[Arg],
        _input: Option<&PipeValue>,
    ) -> Result<CommandOutput, ExecutionError> {
        let q = primary.unwrap_or("").to_string();
        REPL_STATE.with(|s| s.borrow_mut().query = Some(q.clone()));
        Ok(CommandOutput {
            stdout: Some(format!("query set: {q}\n").into_bytes()),
            stderr: None,
            success: true,
        })
    }
}

struct ContextCmd;
impl SlashCommand for ContextCmd {
    fn name(&self) -> &str {
        "context"
    }
    fn methods(&self) -> &[MethodDef] {
        &[]
    }
    fn execute(
        &self,
        primary: Option<&str>,
        _args: &[Arg],
        _input: Option<&PipeValue>,
    ) -> Result<CommandOutput, ExecutionError> {
        let ctx = primary.unwrap_or("").to_string();
        REPL_STATE.with(|s| s.borrow_mut().context = Some(ctx.clone()));
        Ok(CommandOutput {
            stdout: Some(format!("context set ({} bytes)\n", ctx.len()).into_bytes()),
            stderr: None,
            success: true,
        })
    }
}

struct ContextFileCmd;
impl SlashCommand for ContextFileCmd {
    fn name(&self) -> &str {
        "context-file"
    }
    fn methods(&self) -> &[MethodDef] {
        &[]
    }
    fn execute(
        &self,
        primary: Option<&str>,
        _args: &[Arg],
        _input: Option<&PipeValue>,
    ) -> Result<CommandOutput, ExecutionError> {
        let path = primary
            .ok_or_else(|| ExecutionError::Runner("/context-file requires a path".into()))?;
        let content = std::fs::read_to_string(path)
            .map_err(|e| ExecutionError::Runner(format!("read {path}: {e}")))?;
        let len = content.len();
        REPL_STATE.with(|s| s.borrow_mut().context = Some(content));
        Ok(CommandOutput {
            stdout: Some(format!("context loaded ({len} bytes)\n").into_bytes()),
            stderr: None,
            success: true,
        })
    }
}

struct StatusCmd;
impl SlashCommand for StatusCmd {
    fn name(&self) -> &str {
        "status"
    }
    fn methods(&self) -> &[MethodDef] {
        &[]
    }
    fn execute(
        &self,
        _primary: Option<&str>,
        _args: &[Arg],
        _input: Option<&PipeValue>,
    ) -> Result<CommandOutput, ExecutionError> {
        let out = REPL_STATE.with(|s| {
            let s = s.borrow();
            let q = s.query.as_deref().unwrap_or("(not set)");
            let ctx = match &s.context {
                Some(c) => format!("({} bytes)", c.len()),
                None => "(not set)".into(),
            };
            format!("query:   {q}\ncontext: {ctx}\n")
        });
        Ok(CommandOutput {
            stdout: Some(out.into_bytes()),
            stderr: None,
            success: true,
        })
    }
}

struct ClearCmd;
impl SlashCommand for ClearCmd {
    fn name(&self) -> &str {
        "clear"
    }
    fn methods(&self) -> &[MethodDef] {
        &[]
    }
    fn execute(
        &self,
        _primary: Option<&str>,
        _args: &[Arg],
        input: Option<&PipeValue>,
    ) -> Result<CommandOutput, ExecutionError> {
        let _ = input;
        REPL_STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.query = None;
            s.context = None;
        });
        Ok(CommandOutput {
            stdout: Some(b"cleared\n".to_vec()),
            stderr: None,
            success: true,
        })
    }
}

struct HelpCmd;
impl SlashCommand for HelpCmd {
    fn name(&self) -> &str {
        "help"
    }
    fn methods(&self) -> &[MethodDef] {
        &[]
    }
    fn execute(
        &self,
        _primary: Option<&str>,
        _args: &[Arg],
        _input: Option<&PipeValue>,
    ) -> Result<CommandOutput, ExecutionError> {
        let help = "\
/query(<text>)          Set the query to run
/context(<text>)        Set context inline
/context-file(<path>)   Load context from a file
/run                    Execute the RLM with current query + context
/status                 Show current query and context
/clear                  Reset query and context
/help                   Show this message
/quit                   Exit
";
        Ok(CommandOutput {
            stdout: Some(help.as_bytes().to_vec()),
            stderr: None,
            success: true,
        })
    }
}

// /run is special — it needs the Rlm instance, so we pass it at construction.
pub struct RunCmd {
    pub rlm: Arc<tokio::sync::Mutex<Rlm>>,
}

static RUN_METHODS: &[MethodDef] = &[MethodDef::with_value("depth"), MethodDef::flag("verbose")];

impl SlashCommand for RunCmd {
    fn name(&self) -> &str {
        "run"
    }
    fn methods(&self) -> &[MethodDef] {
        RUN_METHODS
    }
    fn execute(
        &self,
        _primary: Option<&str>,
        args: &[Arg],
        _input: Option<&PipeValue>,
    ) -> Result<CommandOutput, ExecutionError> {
        let (query, context) = REPL_STATE.with(|s| {
            let s = s.borrow();
            (s.query.clone(), s.context.clone().unwrap_or_default())
        });

        let query = query.ok_or_else(|| {
            ExecutionError::Runner("no query set — use /query(<text>) first".into())
        })?;

        // Re-configure depth/verbose from args if provided.
        let verbose = args.iter().any(|a| a.name == "verbose");
        let max_depth: usize = args
            .iter()
            .find(|a| a.name == "depth")
            .and_then(|a| a.value.as_deref())
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        let rlm = Arc::clone(&self.rlm);
        let answer = tokio::runtime::Handle::current()
            .block_on(async move {
                let mut r = rlm.lock().await;
                r.verbose = verbose;
                r.max_depth = max_depth;
                r.run(&query, &context).await
            })
            .map_err(|e| ExecutionError::Runner(format!("rlm error: {e}")))?;

        Ok(CommandOutput {
            stdout: Some(format!("{answer}\n").into_bytes()),
            stderr: None,
            success: true,
        })
    }
}

// ---------------------------------------------------------------------------
// Slash command metadata — single source of truth for completion / hints
// ---------------------------------------------------------------------------

const COMMANDS: &[(&str, &str)] = &[
    ("/query(<text>)", "Set the query to run"),
    ("/context(<text>)", "Set context inline"),
    ("/context-file(<path>)", "Load context from a file"),
    ("/run", "Execute the RLM with current query + context"),
    ("/status", "Show current query and context"),
    ("/clear", "Reset query and context"),
    ("/help", "Show available commands"),
    ("/quit", "Exit the REPL"),
];

// ---------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------

struct MlrsPrompt;

impl Prompt for MlrsPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<'_, str> {
        "mlrs> ".into()
    }
    fn render_prompt_right(&self) -> std::borrow::Cow<'_, str> {
        "".into()
    }
    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> std::borrow::Cow<'_, str> {
        "".into()
    }
    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<'_, str> {
        "::: ".into()
    }
    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> std::borrow::Cow<'_, str> {
        let indicator = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        format!("({}reverse-search: {}) ", indicator, history_search.term).into()
    }
}

// ---------------------------------------------------------------------------
// Completer — fires on any input starting with '/'
// ---------------------------------------------------------------------------

struct SlashCompleter;

impl Completer for SlashCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        if !line.starts_with('/') {
            return vec![];
        }
        // Match against the bare command name (strip args signature for matching)
        let prefix = &line[..pos];
        COMMANDS
            .iter()
            .filter(|(cmd, _)| {
                // Strip the args portion for prefix matching: "/query(<text>)" -> "/query"
                let bare = cmd.split('(').next().unwrap_or(cmd);
                bare.starts_with(prefix) || cmd.starts_with(prefix)
            })
            .map(|(cmd, desc)| {
                // Complete to the bare command name (without arg signature)
                let bare = cmd.split('(').next().unwrap_or(cmd).to_string();
                Suggestion {
                    value: bare,
                    display_override: None,
                    description: Some(desc.to_string()),
                    style: None,
                    extra: None,
                    span: Span::new(0, pos),
                    append_whitespace: false,
                    match_indices: None,
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Hinter — inline fish-style ghost text
// ---------------------------------------------------------------------------

struct SlashHinter {
    current_hint: String,
}

impl Hinter for SlashHinter {
    fn handle(
        &mut self,
        line: &str,
        _pos: usize,
        _history: &dyn reedline::History,
        use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.current_hint = String::new();
        if !line.starts_with('/') || line.len() < 2 {
            return String::new();
        }
        for (cmd, _) in COMMANDS {
            let bare = cmd.split('(').next().unwrap_or(cmd);
            if bare.starts_with(line) && bare.len() > line.len() {
                let hint = bare[line.len()..].to_string();
                self.current_hint = hint.clone();
                if use_ansi_coloring {
                    return format!("\x1b[2m{hint}\x1b[0m");
                }
                return hint;
            }
        }
        String::new()
    }

    fn complete_hint(&self) -> String {
        self.current_hint.clone()
    }

    fn next_hint_token(&self) -> String {
        // Return up to the first word boundary in the hint
        self.current_hint
            .split_once(|c: char| c == '(' || c.is_whitespace())
            .map(|(token, _)| token.to_string())
            .unwrap_or_else(|| self.current_hint.clone())
    }
}

// ---------------------------------------------------------------------------
// Highlighter — colour the command name vs arguments
// ---------------------------------------------------------------------------

struct SlashHighlighter;

impl Highlighter for SlashHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        use nu_ansi_term::{Color, Style};
        let mut styled = StyledText::new();
        if line.starts_with('/') {
            let split_at = line.find(['(', ' ']).unwrap_or(line.len());
            styled.push((
                Style::new().bold().fg(Color::Blue),
                line[..split_at].to_string(),
            ));
            if split_at < line.len() {
                styled.push((Style::new(), line[split_at..].to_string()));
            }
        } else {
            styled.push((Style::new(), line.to_string()));
        }
        styled
    }
}

// ---------------------------------------------------------------------------
// REPL loop
// ---------------------------------------------------------------------------

pub fn run(rlm: Rlm) -> Result<()> {
    let rlm = Arc::new(tokio::sync::Mutex::new(rlm));
    let mut registry = CommandRegistry::new(SlenvLoader::empty());
    registry.register(Box::new(QueryCmd));
    registry.register(Box::new(ContextCmd));
    registry.register(Box::new(ContextFileCmd));
    registry.register(Box::new(StatusCmd));
    registry.register(Box::new(ClearCmd));
    registry.register(Box::new(HelpCmd));
    registry.register(Box::new(RunCmd { rlm }));
    let executor = Executor::new(registry);

    let mut editor = Reedline::create()
        .with_completer(Box::new(SlashCompleter))
        .with_hinter(Box::new(SlashHinter {
            current_hint: String::new(),
        }))
        .with_highlighter(Box::new(SlashHighlighter))
        .with_partial_completions(true)
        .with_edit_mode(Box::new(Emacs::default()));

    let prompt = MlrsPrompt;

    println!("mlrs interactive REPL — type /help for commands, /quit to exit");
    println!("Tip: press Tab after '/' to see available commands.");

    loop {
        match editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line == "/quit" || line == "/exit" {
                    break;
                }
                match slash_lang::parser::parse(line) {
                    Err(e) => eprintln!("parse error: {e}"),
                    Ok(prog) => match executor.execute(&prog) {
                        Err(e) => eprintln!("error: {e:?}"),
                        Ok(Some(PipeValue::Bytes(b))) => {
                            print!("{}", String::from_utf8_lossy(&b))
                        }
                        Ok(_) => {}
                    },
                }
            }
            Ok(Signal::CtrlC) => {
                eprintln!("^C");
                continue;
            }
            Ok(Signal::CtrlD) => break,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("readline error: {e}");
                break;
            }
        }
    }

    Ok(())
}
