use anyhow::Ok;
use clap::Parser;
use std::fs::File;
use std::io::Write;
use std::{
    fs,
    sync::{Arc, Mutex},
};
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, Instance, Memory, MemoryType, Module, Store, imports,
};

/// CLI to run compiled rwetac WASM
#[derive(Parser)]
struct Cli {
    /// Path to the compiled WASM file
    wasm_file: String,
}

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
    // Align to 8 bytes: the largest Eta scalar is an i64, so an 8-byte boundary
    // is what the hardware needs. Real allocators usually align to 16 instead,
    // for SIMD types and long double.
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

fn view_array(
    memory: &Memory,
    store: &Store,
    array_addr: usize,
    array_len: usize,
    elem_size: usize,
) -> Result<(), anyhow::Error> {
    println!(
        "Reading array of length {} ({} bytes/element) from address {}",
        array_len, elem_size, array_addr
    );
    // Get a view into the memory. This is done once for efficiency.
    let view = memory.view(store);

    // Loop through each element of the array.
    for i in 0..array_len {
        // Calculate the memory offset for the current element.
        let offset = (array_addr + (i * elem_size)) as u64;

        // Use a match statement to handle different element sizes correctly.
        match elem_size {
            8 => {
                let mut buf = [0u8; 8];
                view.read(offset, &mut buf)?;
                let val = u64::from_le_bytes(buf);
                println!("[Index {} at offset {}]: {}", i, offset, val);
            }
            4 => {
                let mut buf = [0u8; 4];
                view.read(offset, &mut buf)?;
                let val = u32::from_le_bytes(buf);
                println!("[Index {} at offset {}]: {}", i, offset, val);
            }
            _ => {
                eprintln!("Error: Unsupported element size: {}", elem_size);
                // Stop the loop if we encounter an unsupported size.
                break;
            }
        }
    }
    Ok(())
}

/// Dumps a region of Wasm memory to a binary file.
///
/// # Arguments
/// * `memory` - A reference to the Wasm Memory object.
/// * `store` - A reference to the Wasmer Store.
/// * `start_address` - The starting memory address (offset) to read from.
/// * `length` - The number of bytes to read.
/// * `output_path` - The path to the file where the memory will be saved.
fn dump_memory_region(
    memory: &Memory,
    store: &Store,
    start_address: u64,
    length: u64,
    output_path: &str,
) -> Result<(), anyhow::Error> {
    println!(
        "Dumping {} bytes from address {} to '{}'...",
        length, start_address, output_path
    );

    // Get a view into the memory.
    let view = memory.view(store);

    // Safety check: ensure the requested region is within the memory bounds.
    // memory.data_size() returns the size in bytes.
    if start_address + length > view.data_size() {
        anyhow::bail!(
            "Memory access out of bounds: Attempted to read {} bytes from address {} but memory size is only {} bytes.",
            length,
            start_address,
            view.data_size()
        );
    }

    // Create a buffer on the heap to hold the memory data.
    // The cast to `usize` is safe because we've bounded it against memory size,
    // which is itself limited by the host's address space.
    let mut buffer = vec![0u8; length as usize];

    // Read the entire region into the buffer in one operation.
    view.read(start_address, &mut buffer)?;

    // Create the output file.
    let mut file = File::create(output_path)?;

    // Write the entire buffer to the file.
    file.write_all(&buffer)?;

    println!("Successfully exported memory to '{}'.", output_path);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load WASM bytes
    let wasm_bytes = fs::read(&cli.wasm_file)?;

    // Create store and compile module
    let mut store = Store::default();
    let module = Module::new(&store, wasm_bytes)?;

    let heap_ptr = Arc::new(Mutex::new(0u32));
    let env = FunctionEnv::new(
        &mut store,
        AllocatorEnv {
            memory: None,
            heap_ptr: heap_ptr.clone(),
        },
    );
    // Define malloc import
    let eta_malloc_func = Function::new_typed_with_env(&mut store, &env, eta_malloc);

    let _memory = Memory::new(&mut store, MemoryType::new(1, None, false))?;

    // Build import object
    let import_object = imports! {
        "env" => {
            // "memory" => memory.clone(),
            "eta_malloc" => eta_malloc_func,
        }
    };

    // Instantiate
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Access memory and __heap_base
    let heap_base = instance
        .exports
        .get_global("__heap_base")?
        .get(&mut store)
        .i32()
        .expect("`__heap_base` global not found or not an i32");

    // Align to 8, matching eta_malloc
    let start_heap = (heap_base + 7) & !7;
    println!("Start of heap: {}", start_heap);
    let memory = instance.exports.get_memory("memory")?.clone();
    // Set memory and heap pointer in the env
    {
        let mut env_mut = env.clone().into_mut(&mut store);
        env_mut.data_mut().memory = Some(memory.clone());
        *env_mut.data_mut().heap_ptr.lock().unwrap() = heap_base as u32;
    }

    // Optional: call the entry function `_start` if present
    let start = instance.exports.get_function("main").unwrap();
    {
        let results = start.call(&mut store, &[])?;
        println!("Result : {:?}", results);
    }

    // Here is some code to read out the memory after executoin
    let env_ref = env.clone().into_mut(&mut store);
    println!(
        "Value of the heap pointer: {:?}",
        env_ref.data().heap_ptr.lock().unwrap()
    );

    view_array(&memory, &store, start_heap as usize, 17, 8)?;
    dump_memory_region(
        &memory,
        &store,
        start_heap as u64, // Start address
        200,               // Length in bytes
        "memory_dump.bin", // Output file name
    )?;
    Ok(())
}
