use rstest::rstest;
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, Instance, Memory, Module, Store, Value, imports,
};

// Uses compiler to compile and wasmer to run the program

use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

/// Environment passed into `eta_malloc` to manage allocation state
#[derive(Clone)]
pub struct AllocatorEnv {
    memory: Option<Memory>, // populated after instantiation
    heap_ptr: Arc<Mutex<u32>>,
}

// We keep the memory reference here to know if we need to grow the memory
// Later on, when we want to free stuff this becomes useful
pub fn eta_malloc(env: FunctionEnvMut<AllocatorEnv>, size: u32) -> u32 {
    let mut heap = env.data().heap_ptr.lock().unwrap();
    let memory = env.data().memory.as_ref().expect("Memory not initialized");
    // Align to 8 bytes, matching eta_malloc in runtime/src/main.rs
    *heap = (*heap + 7) & !7;
    let ptr = *heap;
    let new_ptr = ptr.checked_add(size).expect("overflow");
    println!("Size requested: {} and heap ptr now at: {}", size, new_ptr);
    let mem_bytes = memory.view(&env).data_size() as usize;
    if (new_ptr as usize) >= mem_bytes {
        // let delta_pages = ((new_ptr as usize - mem_bytes) / wasmer::WASM_PAGE_SIZE) + 1;
        // memory.grow(&mut env, delta_pages as u32).expect("memory grow failed");
        panic!(
            "Out of memory: requested {} bytes, only {} available",
            size,
            mem_bytes - ptr as usize
        );
    }
    *heap = new_ptr;
    ptr
}

fn run_wasm(src_path: &Path, opt: bool) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let pipeline = opt.then(rwetac::setting::Pipeline::default_full);
    rwetac::compile::compile(
        rwetac::setting::Stage::Codegen,
        false,
        pipeline,
        false,
        src_path,
    )?;

    // Get wat_path
    let wat_path = src_path.with_extension("wat");
    let wat_code = fs::read_to_string(&wat_path)?;
    // 3. Load WAT into Wasmer
    let mut store = Store::default();
    let module = Module::new(&store, &wat_code)?;

    let heap_ptr = Arc::new(Mutex::new(0u32));
    let env = FunctionEnv::new(
        &mut store,
        crate::AllocatorEnv {
            memory: None,
            heap_ptr: heap_ptr.clone(),
        },
    );
    // Define malloc import
    let eta_malloc_func = Function::new_typed_with_env(&mut store, &env, eta_malloc);

    let import_object = imports! {
        "env" => {
            // "memory" => memory.clone(),
            "eta_malloc" => eta_malloc_func,
        }
    };
    // 4. Instantiate (no imports yet, adjust if your program needs them)
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Access memory and __heap_base
    let heap_base = instance
        .exports
        .get_global("__heap_base")?
        .get(&mut store)
        .i32()
        .expect("`__heap_base` global not found or not an i32");

    let memory = instance.exports.get_memory("memory")?.clone();
    // Set memory and heap pointer in the env
    {
        let mut env_mut = env.clone().into_mut(&mut store);
        env_mut.data_mut().memory = Some(memory.clone());
        *env_mut.data_mut().heap_ptr.lock().unwrap() = heap_base as u32;
    }

    // 5. Look up `_start` or your main function
    let main_func = instance.exports.get_function("main")?;
    // 6. Call it
    let results = main_func.call(&mut store, &[]);
    //Clean up
    fs::remove_file(&wat_path)?;

    Ok(results?.to_vec())
}

/// True when the error is a WASM trap rather than a compile or setup failure.
fn is_trap(e: &(dyn std::error::Error + 'static)) -> bool {
    e.downcast_ref::<wasmer::RuntimeError>().is_some()
}

#[derive(Debug, PartialEq)]
enum Expectation {
    Value(i64),
    /// Aborts at run time; division by zero is the only way to reach this today.
    Trap,
}

/// Reads the `// EXPECT:` directive out of an `.eta` source.
///
/// The oracle lives with the program, not in this file, so adding a test case
/// is adding one `.eta` file -- which is what lets students drop in their own
/// benchmark programs and have them graded.
fn expected_value(src_path: &Path) -> Expectation {
    let src = fs::read_to_string(src_path).expect("could not read source");
    let name = src_path.file_name().unwrap().to_string_lossy();
    for line in src.lines() {
        if let Some(rest) = line.trim().strip_prefix("// EXPECT:") {
            let rest = rest.trim();
            if rest == "trap" {
                return Expectation::Trap;
            }
            return Expectation::Value(
                rest.parse()
                    .unwrap_or_else(|e| panic!("{name}: malformed EXPECT directive {rest:?}: {e}")),
            );
        }
    }
    panic!("{name}: no `// EXPECT: <int>` or `// EXPECT: trap` directive")
}

/// Integer results are compared numerically, so a `bool` (i32) and an `int`
/// (i64) can share one directive syntax.
fn as_i64(v: &Value) -> i64 {
    match v {
        Value::I32(i) => *i as i64,
        Value::I64(i) => *i,
        other => panic!("expected an integer result, got {other:?}"),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::I32(_) => "i32",
        Value::I64(_) => "i64",
        other => panic!("expected an integer result, got {other:?}"),
    }
}

// rstest is pretty powerful. The fixture stuff could be used for the compiler as well to create the symbol table
#[rstest]
fn eta_integration_tests(#[files("tests/integration_tests/*.eta")] path: std::path::PathBuf) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter) // or your target level
        .try_init();

    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let expected = expected_value(&path);
    let unopt_res = run_wasm(&path, false);
    let opt_res = run_wasm(&path, true);

    if expected == Expectation::Trap {
        // An optimiser that folds the trapping operation away is a miscompile,
        // so both builds have to trap.
        for (label, res) in [("unoptimised", &unopt_res), ("optimised", &opt_res)] {
            match res {
                Ok(v) => panic!("{name}: expected a trap {label}, returned {v:?}"),
                Err(e) if !is_trap(e.as_ref()) => {
                    panic!("{name}: expected a trap {label}, failed instead: {e}")
                }
                Err(_) => (),
            }
        }
        return;
    }
    let Expectation::Value(expected) = expected else {
        unreachable!()
    };

    let unopt = unopt_res.unwrap();
    let opt = opt_res.unwrap();

    assert_eq!(
        unopt.len(),
        1,
        "{name}: expected one return value, got {unopt:?}"
    );
    assert_eq!(
        opt.len(),
        1,
        "{name}: expected one return value, got {opt:?}"
    );

    assert_eq!(
        as_i64(&unopt[0]),
        expected,
        "{name}: wrong result unoptimised"
    );
    assert_eq!(
        as_i64(&opt[0]),
        expected,
        "{name}: wrong result optimised -- the optimiser changed the program's meaning"
    );
    // Catches an optimiser that returns the right number in the wrong width.
    assert_eq!(
        type_name(&unopt[0]),
        type_name(&opt[0]),
        "{name}: optimiser changed the result type"
    );
}
