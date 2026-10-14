(module
  (func $add (param $a i64) (param $b i64) (result i64)
    local.get $a
    local.get $b
    i64.add)
  (func $main (result i64)
    i64.const 3
    i32.const 4
    call $add)
  (export "main" (func $main)))
