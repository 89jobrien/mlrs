use std::cell::RefCell;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

use anyhow::Result;
use slash_core::{
    command::{MethodDef, SlashCommand},
    env::SlenvLoader,
    executor::{CommandOutput, CommandRunner, Execute, ExecutionError, Executor, PipeValue},
    registry::CommandRegistry,
};
use slash_lang::parser::ast::{Arg, Command};

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

    let stdin = io::stdin();
    let stdout = io::stdout();

    println!("mlrs interactive REPL — type /help for commands, /quit to exit");

    loop {
        print!("mlrs> ");
        stdout.lock().flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }
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
                Ok(Some(PipeValue::Bytes(b))) => print!("{}", String::from_utf8_lossy(&b)),
                Ok(_) => {}
            },
        }
    }

    Ok(())
}
