use anyhow::Result;
use rhai::{Engine, Scope};

use crate::protocol::StepResult;

/// Build a Rhai engine with all RLM context functions registered.
///
/// The `ctx` string is stored as a Rhai variable in the returned `Scope`.
/// Registered functions operate on a thread-local copy of the context so
/// Rhai closures can call them without borrowing issues.
pub fn build_engine(ctx: String) -> (Engine, Scope<'static>) {
    use std::sync::{Arc, Mutex};

    let ctx_arc: Arc<Mutex<String>> = Arc::new(Mutex::new(ctx.clone()));

    let mut engine = Engine::new();

    // ctx_len() -> int
    {
        let c = Arc::clone(&ctx_arc);
        engine.register_fn("ctx_len", move || c.lock().unwrap().len() as i64);
    }

    // ctx_slice(start: int, end: int) -> String
    {
        let c = Arc::clone(&ctx_arc);
        engine.register_fn("ctx_slice", move |start: i64, end: i64| {
            let s = c.lock().unwrap();
            let start = (start as usize).min(s.len());
            let end = (end as usize).min(s.len());
            s[start..end].to_string()
        });
    }

    // ctx_grep(pattern: String) -> String
    {
        let c = Arc::clone(&ctx_arc);
        engine.register_fn("ctx_grep", move |pattern: String| {
            let s = c.lock().unwrap();
            match regex::Regex::new(&pattern) {
                Ok(re) => s
                    .lines()
                    .filter(|l| re.is_match(l))
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(e) => format!("regex error: {e}"),
            }
        });
    }

    // print_cell(msg: String) — appended to cell output via a shared buffer.
    // The buffer is drained by execute_script after the script runs.
    let print_buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let buf = Arc::clone(&print_buf);
        engine.register_fn("print_cell", move |msg: String| {
            buf.lock().unwrap().push(msg);
        });
    }

    // `final` is a reserved keyword in Rhai — register as `done` as the exit signal.
    // The system prompt instructs the model to call `done(answer)`.
    let final_buf: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    {
        let buf = Arc::clone(&final_buf);
        engine.register_fn("done", move |answer: String| {
            *buf.lock().unwrap() = Some(answer);
        });
    }

    // Store the Arc refs on the engine's user data is not supported in rhai 1.x without
    // a wrapper struct. Instead we return a scope with the ctx variable set, and expose
    // print_buf / final_buf handles through thread-locals accessed by execute_script.
    //
    // SAFETY: We use a thread-local registry keyed by the engine pointer address as a
    // workaround for rhai's lack of arbitrary user-data storage.
    PRINT_BUF.with(|pb| *pb.borrow_mut() = Some(Arc::clone(&print_buf)));
    FINAL_BUF.with(|fb| *fb.borrow_mut() = Some(Arc::clone(&final_buf)));

    let mut scope = Scope::new();
    scope.push("ctx", ctx);

    (engine, scope)
}

thread_local! {
    static PRINT_BUF: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>> =
        std::cell::RefCell::new(None);
    static FINAL_BUF: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<Option<String>>>>> =
        std::cell::RefCell::new(None);
}

/// Execute a Rhai script and return the `StepResult`.
///
/// On compile/runtime error the error message is returned as `Continue` so the
/// model can self-correct (the caller counts retries).
pub fn execute_script(engine: &Engine, scope: &mut Scope, script: &str) -> Result<StepResult> {
    let exec_result = engine.eval_with_scope::<rhai::Dynamic>(scope, script);

    // Drain print buffer regardless of success/failure.
    let printed = PRINT_BUF.with(|pb| {
        pb.borrow()
            .as_ref()
            .map(|arc| arc.lock().unwrap().drain(..).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    });

    // Check if done() was called.
    let final_answer = FINAL_BUF.with(|fb| {
        fb.borrow()
            .as_ref()
            .and_then(|arc| arc.lock().unwrap().take())
    });

    if let Some(answer) = final_answer {
        return Ok(StepResult::Final(answer));
    }

    match exec_result {
        Ok(val) => {
            let mut output = printed;
            let val_str = val.to_string();
            if !val_str.is_empty() && val_str != "()" {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&val_str);
            }
            Ok(StepResult::Continue(output))
        }
        Err(e) => Ok(StepResult::Continue(format!("script error: {e}"))),
    }
}
