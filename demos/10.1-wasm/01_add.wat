(module
  (func $add (param $a i64)
             (param $b i64)
             (result i64)
    local.get $b
    local.get $b
    i64.const 5
    i64.add
    i64.add)
  (export "add" (func $add)))
