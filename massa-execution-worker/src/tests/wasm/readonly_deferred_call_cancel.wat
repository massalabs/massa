;; Rebuild: wat2wasm readonly_deferred_call_cancel.wat -o readonly_deferred_call_cancel.wasm
(module
  (import "massa" "assembly_script_deferred_call_cancel"
    (func $deferred_call_cancel (param i32)))

  (memory (export "memory") 1)

  ;; One parameter allocation per instance. Its UTF-16LE bytes share the AS
  ;; string/buffer layout: byte length stored four bytes before the data.
  (func (export "__new") (param $size i32) (param $class_id i32) (result i32)
    i32.const 1020
    local.get $size
    i32.store
    i32.const 1024)

  (func (export "cancel") (param $deferred_call_id i32)
    local.get $deferred_call_id
    call $deferred_call_cancel)
  (func (export "cancel_then_trap") (param $deferred_call_id i32)
    local.get $deferred_call_id
    call $deferred_call_cancel
    unreachable))
