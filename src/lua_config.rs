use std::time::Instant;
use mlua::prelude::*;
use event_chains::{ChainableEvent, EventChain, EventContext, EventResult};

// ============================================================================
// REGISTERED EVENTS (Rust implementations)
// ============================================================================

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

// ============================================================================
// ENUM WRAPPER (allows EventChain to work with any registered event)
// ============================================================================

enum RegisteredEvent {
    Increment(IncrementEvent),
    Append(AppendEvent),
}

impl ChainableEvent for RegisteredEvent {
    fn execute(&self, context: &mut EventContext) -> EventResult<()> {
        match self {
            RegisteredEvent::Increment(e) => e.execute(context),
            RegisteredEvent::Append(e) => e.execute(context),
        }
    }
    fn name(&self) -> &str {
        match self {
            RegisteredEvent::Increment(e) => e.name(),
            RegisteredEvent::Append(e) => e.name(),
        }
    }
}

fn main() -> LuaResult<()> {
    println!("{}\n", "=".repeat(70));
    println!("HARDCODED RUST CHAIN (baseline):");
    println!("{}\n", "=".repeat(70));

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
    // LUA-DEFINED CHAIN (Lua selects events, Rust backend executes)
    // ========================================================================
    println!("{}\n", "=".repeat(70));
    println!("LUA-DEFINED CHAIN (Lua configures, EventChains executes):");
    println!("{}\n", "=".repeat(70));

    let lua = Lua::new();

    // Lua script: select which events to run and context
    let script = r#"
return {
  context = {
    counter = 0,
    message = "start"
  },
  events = {
    "increment",
    "append"
  }
}
"#;

    // === PARSE LUA ===
    let lua_parse_start = Instant::now();
    let chain_def: LuaTable = lua.load(script).eval()?;
    let lua_parse_duration = lua_parse_start.elapsed();
    println!("Lua parsing time: {:?}", lua_parse_duration);

    // === EXTRACT CONTEXT FROM LUA ===
    let context_table: LuaTable = chain_def.get("context")?;
    let mut context = EventContext::new();

    for pair in context_table.pairs::<String, LuaValue>() {
        let (key, value) = pair?;
        match value {
            LuaValue::Integer(i) => context.set(&key, i),
            LuaValue::String(s) => context.set(&key, s.to_string_lossy().to_string()),
            _ => {}
        }
    }
    println!("Context extracted from Lua");

    // === BUILD EVENTCHAIN FROM LUA EVENT NAMES ===
    let lua_build_start = Instant::now();
    let event_names: Vec<String> = chain_def.get("events")?;
    let mut lua_chain = EventChain::new();

    for name in event_names {
        let event: RegisteredEvent = match name.as_str() {
            "increment" => RegisteredEvent::Increment(IncrementEvent),
            "append" => RegisteredEvent::Append(AppendEvent),
            _ => return Err(LuaError::RuntimeError(format!("Unknown event: {}", name))),
        };
        lua_chain = lua_chain.event(event);
    }
    let lua_build_duration = lua_build_start.elapsed();
    println!("EventChain built from Lua in: {:?}", lua_build_duration);

    // === EXECUTE THROUGH EVENTCHAINS ===
    let lua_exec_start = Instant::now();
    let result = lua_chain.execute(&mut context);
    let lua_exec_duration = lua_exec_start.elapsed();

    println!("EventChains execution time: {:?}", lua_exec_duration);
    println!("Final counter: {:?}", context.get::<i64>("counter"));
    println!("Final message: {:?}", context.get::<String>("message"));
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

    // Lua 100x (reuse chain definition, rebuild from scratch each time)
    let lua_repeated_start = Instant::now();
    for _ in 0..iterations {
        let chain_def: LuaTable = lua.load(script).eval()?;
        let context_table: LuaTable = chain_def.get("context")?;
        let mut context = EventContext::new();

        for pair in context_table.pairs::<String, LuaValue>() {
            let (key, value) = pair?;
            match value {
                LuaValue::Integer(i) => context.set(&key, i),
                LuaValue::String(s) => context.set(&key, s.to_string_lossy().to_string()),
                _ => {}
            }
        }

        let event_names: Vec<String> = chain_def.get("events")?;
        let mut lua_chain = EventChain::new();

        for name in event_names {
            let event: RegisteredEvent = match name.as_str() {
                "increment" => RegisteredEvent::Increment(IncrementEvent),
                "append" => RegisteredEvent::Append(AppendEvent),
                _ => return Err(LuaError::RuntimeError(format!("Unknown event: {}", name))),
            };
            lua_chain = lua_chain.event(event);
        }

        let _result = lua_chain.execute(&mut context);
    }
    let lua_repeated_duration = lua_repeated_start.elapsed();
    let lua_per_iter = lua_repeated_duration.as_micros() / iterations as u128;

    println!("Lua ({}x): {:?}", iterations, lua_repeated_duration);
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

    println!("Per-Iteration (100 runs):");
    println!("  Hardcoded: {:.2}µs", hardcoded_per_iter as f64);
    println!("  Lua: {:.2}µs", lua_per_iter as f64);
    println!("  Overhead: {:.2}%\n", repeated_overhead);

    println!("Cost Breakdown (single execution):");
    println!("  Lua parsing: {:.2}µs", lua_parse_duration.as_micros() as f64);
    println!("  EventChain building: {:.2}µs", lua_build_duration.as_micros() as f64);
    println!("  EventChains execution: {:.2}µs", lua_exec_duration.as_micros() as f64);

    Ok(())
}
