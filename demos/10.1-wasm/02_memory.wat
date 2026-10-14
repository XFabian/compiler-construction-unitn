(module
  (func $run (param $ptr i32)
             (result i32)
    (local $my_value i32)
    i32.const 42
    local.set $my_value
    local.get $ptr
    local.get $my_value
    i32.store
    local.get $ptr
    i32.load
    i32.const 5
    i32.add)
  (memory $mem 1)
  (export "memory" (memory $mem))
  (export "run" (func $run)))
