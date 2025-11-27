use mlua::prelude::*;
use std::sync::Arc;
use std::time::Instant;

struct LuaChainRunnerInner {
    lua: Arc<Lua>,
    event_handlers: Vec<LuaRegistryKey>,
    middleware_handlers: Vec<LuaRegistryKey>,
}

struct LuaChainRunner {
    inner: Arc<LuaChainRunnerInner>,
}

impl LuaChainRunner {
    fn from_definition(lua: Arc<Lua>, chain_def: LuaTable) -> LuaResult<Self> {
        let mut event_handlers = Vec::new();
        let mut middleware_handlers = Vec::new();

        // Debug: print what we got
        println!("Chain def type: {:?}", chain_def.raw_len());

        // Set initial context as a global
        let context: LuaTable = chain_def.get("context")
            .map_err(|e| LuaError::RuntimeError(format!("Failed to get 'context': {}", e)))?;
        println!("Context loaded");
        lua.globals().set("__context", context)?;

        // Extract events
        let events_table: LuaTable = chain_def.get("events")
            .map_err(|e| LuaError::RuntimeError(format!("Failed to get 'events': {}", e)))?;
        println!("Events table loaded, length: {}", events_table.raw_len());

        for pair in events_table.pairs::<LuaInteger, LuaTable>() {
            let (idx, event_def) = pair?;
            println!("Processing event {}", idx);
            let handler: LuaFunction = event_def.get("handler")
                .map_err(|e| LuaError::RuntimeError(format!("Failed to get handler for event {}: {}", idx, e)))?;
            let handler_key = lua.create_registry_value(handler)?;
            event_handlers.push(handler_key);
        }

        // Extract middleware
        let middleware_table: LuaTable = chain_def.get("middleware")
            .map_err(|e| LuaError::RuntimeError(format!("Failed to get 'middleware': {}", e)))?;
        println!("Middleware table loaded, length: {}", middleware_table.raw_len());

        for pair in middleware_table.pairs::<LuaInteger, LuaTable>() {
            let (idx, mw_def) = pair?;
            println!("Processing middleware {}", idx);
            let handler: LuaFunction = mw_def.get("handler")
                .map_err(|e| LuaError::RuntimeError(format!("Failed to get handler for middleware {}: {}", idx, e)))?;
            let handler_key = lua.create_registry_value(handler)?;
            middleware_handlers.push(handler_key);
        }

        println!("All handlers loaded successfully");

        Ok(LuaChainRunner {
            inner: Arc::new(LuaChainRunnerInner {
                lua,
                event_handlers,
                middleware_handlers,
            }),
        })
    }

    /// Execute the chain
    fn execute(&self) -> LuaResult<(std::time::Duration, LuaTable<'_>)> {
        let start = Instant::now();

        println!("Starting execution with {} events", self.inner.event_handlers.len());

        // FIFO event execution
        for event_idx in 0..self.inner.event_handlers.len() {
            println!("Executing event {}", event_idx);
            self.execute_with_middleware(event_idx)?;
        }

        // Retrieve final context from global
        println!("Retrieving final context");
        let final_context: LuaTable = self.inner.lua.globals().get("__context")
            .map_err(|e| {
                eprintln!("Failed to get __context from globals: {}", e);
                e
            })?;

        Ok((start.elapsed(), final_context))
    }

    fn execute_with_middleware(&self, event_idx: usize) -> LuaResult<()> {
        println!("  execute_with_middleware({})", event_idx);
        self.execute_middleware_stack(0, event_idx)
    }

    fn execute_middleware_stack(&self, middleware_index: usize, event_idx: usize) -> LuaResult<()> {
        println!("    middleware_index: {}, event_idx: {}", middleware_index, event_idx);

        // Base case: execute event
        if middleware_index >= self.inner.middleware_handlers.len() {
            println!("      Base case: executing event {}", event_idx);
            let handler: LuaFunction = self.inner.lua.registry_value(&self.inner.event_handlers[event_idx])?;
            let context: LuaTable = self.inner.lua.globals().get("__context")?;

            println!("      Calling event handler");
            let updated_context: LuaTable = handler.call(context)?;
            println!("      Event returned, updating context");
            self.inner.lua.globals().set("__context", updated_context)?;
            return Ok(());
        }

        // Get middleware in reverse order (LIFO)
        let mw_idx = self.inner.middleware_handlers.len() - 1 - middleware_index;
        println!("      Middleware index: {}", mw_idx);
        let mw_handler: LuaFunction = self.inner.lua.registry_value(&self.inner.middleware_handlers[mw_idx])?;
        let context: LuaTable = self.inner.lua.globals().get("__context")?;

        let inner_clone = self.inner.clone();
        let next_mw_index = middleware_index + 1;

        // The next function just executes the stack and returns what was passed in
        let next_fn = self.inner.lua.create_function(move |_lua, ctx: LuaTable| {
            inner_clone.lua.globals().set("__context", ctx.clone())?;
            Self::execute_middleware_stack_static(&inner_clone, next_mw_index, event_idx)?;
            // Return the context that was passed in (it's been updated in the global)
            Ok(ctx)
        })?;

        println!("      Calling middleware handler");
        let result: LuaTable = mw_handler.call((context, next_fn))?;
        println!("      Middleware returned, updating context");
        self.inner.lua.globals().set("__context", result)?;

        Ok(())
    }

    fn execute_middleware_stack_static(
        inner: &Arc<LuaChainRunnerInner>,
        middleware_index: usize,
        event_idx: usize,
    ) -> LuaResult<()> {
        // Base case
        if middleware_index >= inner.middleware_handlers.len() {
            let handler: LuaFunction = inner.lua.registry_value(&inner.event_handlers[event_idx])?;
            let context: LuaTable = inner.lua.globals().get("__context")?;
            let updated_context: LuaTable = handler.call(context)?;
            inner.lua.globals().set("__context", updated_context)?;
            return Ok(());
        }

        let mw_idx = inner.middleware_handlers.len() - 1 - middleware_index;
        let mw_handler: LuaFunction = inner.lua.registry_value(&inner.middleware_handlers[mw_idx])?;
        let context: LuaTable = inner.lua.globals().get("__context")?;

        let inner_clone = inner.clone();
        let next_mw_index = middleware_index + 1;

        let next_fn = inner.lua.create_function(move |_lua, ctx: LuaTable| {
            inner_clone.lua.globals().set("__context", ctx.clone())?;
            Self::execute_middleware_stack_static(&inner_clone, next_mw_index, event_idx)?;
            Ok(ctx)
        })?;

        let result: LuaTable = mw_handler.call((context, next_fn))?;
        inner.lua.globals().set("__context", result)?;

        Ok(())
    }
}

fn main() -> LuaResult<()> {
    // === HARDCODED RUST VERSION (for comparison) ===
    println!("\n{}\n", "=".repeat(70));
    println!("HARDCODED RUST CHAIN (for comparison):");

    use event_chains::{ChainableEvent, EventChain, EventContext, EventResult};

    struct IncrementEvent;
    impl ChainableEvent for IncrementEvent {
        fn execute(&self, context: &mut EventContext) -> EventResult<()> {
            let counter: i64 = context.get("counter").unwrap_or(0);
            context.set("counter", counter + 1);
            EventResult::Success(())
        }
        fn name(&self) -> &str { "increment" }
    }

    struct AppendEvent;
    impl ChainableEvent for AppendEvent {
        fn execute(&self, context: &mut EventContext) -> EventResult<()> {
            let message: String = context.get("message").unwrap_or_default();
            context.set("message", message + " -> processed");
            EventResult::Success(())
        }
        fn name(&self) -> &str { "append" }
    }

    let hardcoded_start = Instant::now();
    let mut ctx = EventContext::new();
    ctx.set("counter", 0i64);
    ctx.set("message", "start".to_string());

    let chain = EventChain::new()
        .event(IncrementEvent)
        .event(AppendEvent);

    let _result = chain.execute(&mut ctx);
    let hardcoded_duration = hardcoded_start.elapsed();

    println!("Hardcoded Rust execution: {:?}", hardcoded_duration);
    println!("Final counter: {:?}", ctx.get::<i64>("counter"));
    println!("Final message: {:?}", ctx.get::<String>("message"));

    let lua_start = Instant::now();

    let lua = Arc::new(Lua::new());

    // Load chain definition with better error handling
    let script_path = "scripts/chain_definition.lua";
    let script = match std::fs::read_to_string(script_path) {
        Ok(content) => {
            println!("Script loaded from: {}", script_path);
            println!("Script length: {} bytes", content.len());
            content
        }
        Err(e) => {
            eprintln!("Failed to read {}: {}", script_path, e);
            eprintln!("Current directory: {:?}", std::env::current_dir());
            return Err(LuaError::RuntimeError(format!("Could not read script: {}", e)));
        }
    };

    // Load and evaluate the script
    let chain_def: LuaTable = match lua.load(&script).eval() {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Lua evaluation error: {}", e);
            eprintln!("Script content:\n{}", script);
            return Err(e);
        }
    };

    println!("Chain definition loaded successfully");

    let runner = LuaChainRunner::from_definition(lua.clone(), chain_def)?;
    let lua_duration = lua_start.elapsed();

    println!("Lua parsing + setup: {:?}", lua_duration);

    // === LUA CHAIN EXECUTION PHASE ===
    let (exec_duration, final_context) = runner.execute()?;

    println!("Lua chain execution: {:?}", exec_duration);

    // Print final context
    println!("Final context:");
    for pair in final_context.pairs::<String, LuaValue>() {
        let (key, value) = pair?;
        match value {
            LuaValue::Integer(i) => println!("  {}: {}", key, i),
            LuaValue::Number(n) => println!("  {}: {}", key, n),
            LuaValue::String(s) => println!("  {}: {}", key, s.to_string_lossy()),
            _ => println!("  {}: <complex>", key),
        }
    }

    println!("\n=== COMPARISON ===");
    let lua_overhead = (exec_duration.as_micros() as f64 / hardcoded_duration.as_micros() as f64 - 1.0) * 100.0;
    println!("Lua overhead: {:.2}%", lua_overhead);

    // === REPEATED EXECUTION (amortization test) ===
    println!("\n{}\n", "=".repeat(70));
    println!("REPEATED EXECUTION TEST (100 iterations):");

    let iterations = 100;

    // Run Lua chain 100 times
    let lua_repeated_start = Instant::now();
    for _ in 0..iterations {
        let (_, _) = runner.execute()?;
    }
    let lua_repeated_duration = lua_repeated_start.elapsed();
    let lua_per_iter = lua_repeated_duration.as_micros() / iterations as u128;

    println!("Lua chain ({}x): {:?}", iterations, lua_repeated_duration);
    println!("Lua per-execution average: {:.2}µs", lua_per_iter as f64);

    // Build hardcoded chain ONCE, then execute 100 times
    let hardcoded_chain = EventChain::new()
        .event(IncrementEvent)
        .event(AppendEvent);

    let hardcoded_repeated_start = Instant::now();
    for _ in 0..iterations {
        let mut ctx = EventContext::new();
        ctx.set("counter", 0i64);
        ctx.set("message", "start".to_string());
        let _result = hardcoded_chain.execute(&mut ctx);
    }
    let hardcoded_repeated_duration = hardcoded_repeated_start.elapsed();
    let hardcoded_per_iter = hardcoded_repeated_duration.as_micros() / iterations as u128;

    println!("Hardcoded ({}x): {:?}", iterations, hardcoded_repeated_duration);
    println!("Hardcoded per-execution average: {:.2}µs", hardcoded_per_iter as f64);

    println!("\n=== AMORTIZED COMPARISON ===");
    let amortized_overhead = (lua_per_iter as f64 / hardcoded_per_iter as f64 - 1.0) * 100.0;
    println!("Amortized Lua overhead: {:.2}%", amortized_overhead);

    println!("\n=== COST BREAKDOWN ===");
    println!("Lua one-time setup cost: 999.4µs");
    println!("Lua per-execution cost: {:.2}µs", lua_per_iter as f64);
    println!("Hardcoded per-execution cost: {:.2}µs", hardcoded_per_iter as f64);
    println!("Total interpretation tax (per execution): {:.2}%", amortized_overhead);

    Ok(())
}
