(module
  (type $external (func (param i64) (result i64)))
  (func $foo (type $external) (param i64) (result i64)
    (local i64)
    (local.set 1 (local.get 0))
    (loop $loop (result i64)
      (if (result i64) (i64.eq (local.get 0) (i64.const 0))
          (local.get 1)
          (block (result i64)
             (local.set 1 (i64.add (local.get 1) (local.get 1)))
             (local.set 0 (i64.sub (local.get 0) (i64.const 1)))
             (br $loop)
          )
      )
    )
  )
  (table (;0;) 1 1 funcref)
  ;; (memory (;0;) 1024)
  (global (;0;) (mut i32) (i32.const 1048576))
  (global (;1;) i32 (i32.const 1048576))
  (global (;2;) i32 (i32.const 1048576))
  ;; (export "memory" (memory 0))
  (export "foo_extern" (func $foo))
)