use std::time::Instant;
use mlua::prelude::*;
use event_chains::{ChainableEvent, EventChain, EventContext, EventResult};

// ============================================================================
// THREAD-LOCAL LUA CONTEXT
// ============================================================================
// mlua::Lua is NOT Send + Sync, so we use thread-local storage
// This works within a single thread (production scenario: one thread per Lua chain)

thread_local! {
    static LUA_VM: std::cell::RefCell<Option<Lua>> = std::cell::RefCell::new(None);
}

fn init_lua_vm(script: &str) -> LuaResult<()> {
    LUA_VM.with(|lua_ref| {
        let lua = Lua::new();
        let chain_def: LuaTable = lua.load(script).eval()?;

        // Initialize context in globals
        let context_table: LuaTable = chain_def.get("context")?;
        lua.globals().set("__context", context_table)?;
        lua.globals().set("__chain_def", chain_def)?;

        *lua_ref.borrow_mut() = Some(lua);
        Ok(())
    })
}

fn with_lua<F, R>(f: F) -> LuaResult<R>
where
    F: FnOnce(&Lua) -> LuaResult<R>,
{
    LUA_VM.with(|lua_ref| {
        let borrowed = lua_ref.borrow();
        match borrowed.as_ref() {
            Some(lua) => f(lua),
            None => Err(LuaError::external("Lua VM not initialized")),
        }
    })
}

// ============================================================================
// LUA EVENT WRAPPER (implements ChainableEvent)
// ============================================================================
// Stores only the handler function name; retrieves handler at execution time

struct LuaEventWrapper {
    name: String,
    handler_key: String,  // Key to retrieve handler from Lua globals
}

impl ChainableEvent for LuaEventWrapper {
    fn execute(&self, _context: &mut EventContext) -> EventResult<()> {
        // Call with_lua and convert Result<(), LuaError> to EventResult<()>
        match with_lua(|lua| {
            // Get the handler function from globals
            let handler: LuaFunction = lua.globals().get(self.handler_key.as_str())?;

            // Get context from globals
            let ctx_table: LuaTable = lua.globals().get("__context")?;

            // Call handler with context
            let updated: LuaTable = handler.call(ctx_table)?;

            // Update globals with new context
            lua.globals().set("__context", updated)?;

            Ok(())
        }) {
            Ok(_) => EventResult::Success(()),
            Err(e) => EventResult::Failure(format!("Lua handler execution failed: {}", e)),
        }
    }

    fn name(&self) -> &str { &self.name }
}

fn main() -> LuaResult<()> {
    println!("{}\n", "=".repeat(70));
    println!("HARDCODED RUST CHAIN (baseline):");
    println!("{}\n", "=".repeat(70));

    // Hardcoded events for baseline
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

    let result = chain.execute(&mut ctx);
    let hardcoded_duration = hardcoded_start.elapsed();

    println!("Execution time: {:?}", hardcoded_duration);
    println!("Final counter: {:?}", ctx.get::<i64>("counter"));
    println!("Final message: {:?}", ctx.get::<String>("message"));
    println!("Result: {:?}\n", result.status);

    // ========================================================================
    // LUA-DEFINED EVENTS EXECUTED THROUGH EVENTCHAINS
    // ========================================================================
    println!("{}\n", "=".repeat(70));
    println!("LUA-DEFINED EVENTS (handlers in Lua, executed via EventChains):");
    println!("{}\n", "=".repeat(70));

    // Lua script: define events with their handlers
    let script = r#"
return {
  context = {
    counter = 0,
    message = "start"
  },
  events = {
    {
      name = "increment",
      handler = function(ctx)
        ctx.counter = ctx.counter + 1
        return ctx
      end
    },
    {
      name = "append",
      handler = function(ctx)
        ctx.message = ctx.message .. " -> processed"
        return ctx
      end
    }
  }
}
"#;

    // === PARSE & INITIALIZE LUA ===
    let lua_parse_start = Instant::now();
    init_lua_vm(script)?;
    let lua_parse_duration = lua_parse_start.elapsed();
    println!("Lua parsing & initialization: {:?}", lua_parse_duration);

    // === BUILD EVENTCHAIN FROM LUA EVENT DEFINITIONS ===
    let lua_build_start = Instant::now();

    let lua_chain = with_lua(|lua| {
        let chain_def: LuaTable = lua.globals().get("__chain_def")?;
        let events_table: LuaTable = chain_def.get("events")?;
        let mut chain = EventChain::new();

        for pair in events_table.pairs::<LuaInteger, LuaTable>() {
            let (_, event_def) = pair?;
            let name: String = event_def.get("name")?;
            let handler: LuaFunction = event_def.get("handler")?;

            // Store handler in Lua globals with key like "__handler_increment"
            let handler_key = format!("__handler_{}", name);
            // Use handler_key.as_str() which implements IntoLua
            lua.globals().set(handler_key.as_str(), handler)?;

            chain = chain.event(LuaEventWrapper {
                name: name.clone(),
                handler_key,
            });
        }

        Ok(chain)
    })?;

    let lua_build_duration = lua_build_start.elapsed();
    println!("EventChain built from Lua handlers in: {:?}", lua_build_duration);

    // === EXECUTE THROUGH EVENTCHAINS ===
    let lua_exec_start = Instant::now();
    let mut lua_context = EventContext::new();  // Rust context (not used, everything in Lua)
    let result = lua_chain.execute(&mut lua_context);
    let lua_exec_duration = lua_exec_start.elapsed();

    // === RETRIEVE FINAL CONTEXT FROM LUA ===
    let (final_counter, final_message) = with_lua(|lua| {
        let final_ctx: LuaTable = lua.globals().get("__context")?;
        let counter: i64 = final_ctx.get("counter")?;
        let message: String = final_ctx.get("message")?;
        Ok((counter, message))
    })?;

    println!("EventChains execution time: {:?}", lua_exec_duration);
    println!("Final counter: {}", final_counter);
    println!("Final message: {}", final_message);
    println!("Result: {:?}\n", result.status);

    // ========================================================================
    // REPEATED EXECUTION (100 iterations)
    // ========================================================================
    println!("{}\n", "=".repeat(70));
    println!("REPEATED EXECUTION (100 iterations):");
    println!("{}\n", "=".repeat(70));

    let iterations = 100;

    // Hardcoded 100x
    let hardcoded_repeated_start = Instant::now();
    for _ in 0..iterations {
        let mut ctx = EventContext::new();
        ctx.set("counter", 0i64);
        ctx.set("message", "start".to_string());

        let chain = EventChain::new()
            .event(IncrementEvent)
            .event(AppendEvent);

        let _result = chain.execute(&mut ctx);
    }
    let hardcoded_repeated_duration = hardcoded_repeated_start.elapsed();
    let hardcoded_per_iter = hardcoded_repeated_duration.as_micros() / iterations as u128;

    println!("Hardcoded ({}x): {:?}", iterations, hardcoded_repeated_duration);
    println!("Hardcoded per-iteration: {:.2}µs\n", hardcoded_per_iter as f64);

    // Lua 100x (reuse chain, just reset context)
    let lua_repeated_start = Instant::now();
    for _ in 0..iterations {
        // Reset context in Lua globals
        with_lua(|lua| {
            let initial_ctx: LuaTable = lua.create_table()?;
            initial_ctx.set("counter", 0i64)?;
            initial_ctx.set("message", "start")?;
            lua.globals().set("__context", initial_ctx)?;
            Ok::<(), LuaError>(())
        })?;

        // Execute pre-built chain
        let mut lua_context = EventContext::new();
        let _result = lua_chain.execute(&mut lua_context);
    }
    let lua_repeated_duration = lua_repeated_start.elapsed();
    let lua_per_iter = lua_repeated_duration.as_micros() / iterations as u128;

    println!("Lua ({}x, chain reused): {:?}", iterations, lua_repeated_duration);
    println!("Lua per-iteration: {:.2}µs\n", lua_per_iter as f64);

    // ========================================================================
    // FINAL COMPARISON
    // ========================================================================
    println!("{}\n", "=".repeat(70));
    println!("INTERPRETATION TAX ANALYSIS:");
    println!("{}\n", "=".repeat(70));

    let lua_total = (lua_parse_duration.as_micros() + lua_build_duration.as_micros() + lua_exec_duration.as_micros()) as f64;
    let hardcoded = hardcoded_duration.as_micros() as f64;
    let single_overhead = (lua_total / hardcoded - 1.0) * 100.0;
    let repeated_overhead = (lua_per_iter as f64 / hardcoded_per_iter as f64 - 1.0) * 100.0;

    println!("Single Execution:");
    println!("  Hardcoded: {:.2}µs", hardcoded);
    println!("  Lua total (parse + build + exec): {:.2}µs", lua_total);
    println!("  Overhead: {:.2}%\n", single_overhead);

    println!("Per-Iteration (100 runs, chain reused):");
    println!("  Hardcoded: {:.2}µs", hardcoded_per_iter as f64);
    println!("  Lua: {:.2}µs", lua_per_iter as f64);
    println!("  Overhead: {:.2}%\n", repeated_overhead);

    println!("Cost Breakdown (single execution):");
    println!("  Lua parsing: {:.2}µs", lua_parse_duration.as_micros() as f64);
    println!("  EventChain building: {:.2}µs", lua_build_duration.as_micros() as f64);
    println!("  EventChains execution (with Lua handlers): {:.2}µs", lua_exec_duration.as_micros() as f64);

    println!("\n=== KEY INSIGHT ===");
    let per_handler_overhead = (lua_per_iter as i128) - (hardcoded_per_iter as i128);
    println!("With chain reused (production scenario):");
    println!("  Lua handler overhead per execution: {:.2}µs", per_handler_overhead as f64);

    if per_handler_overhead > 0 {
        let setup_cost = (lua_parse_duration.as_micros() + lua_build_duration.as_micros()) as i128;
        let breakeven = setup_cost / per_handler_overhead;
        println!("  Break-even point: ~{} executions", breakeven);
    } else if per_handler_overhead == 0 {
        println!("  Break-even: NONE - Lua is equal speed to hardcoded Rust!");
        println!("  Setup cost is amortized on first execution.");
    } else {
        println!("  Break-even: NEGATIVE - Lua is FASTER than hardcoded Rust!");
        println!("  (EventChain setup overhead dominates in hardcoded benchmark)");
    }

    println!("\n=== ARCHITECTURE NOTES ===");
    println!("This uses thread-local storage because mlua::Lua is NOT Send+Sync.");
    println!("Lua instances are single-threaded; one instance per Lua chain per thread.");
    println!("For multi-threaded use, create separate Lua instances in separate threads.");

    Ok(())
}
