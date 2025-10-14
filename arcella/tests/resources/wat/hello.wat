(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory 1)
  (export "memory" (memory 0))
  (data (i32.const 8) "Hello, world!\n")
  (func $main (export "_start")
    ;; iov structure: address of string, length
    (i32.store (i32.const 0) (i32.const 8))   ;; iov[0].iov_base
    (i32.store (i32.const 4) (i32.const 14))  ;; iov[0].iov_len
    ;; call fd_write to stdout (fd=1)
    (call $fd_write
      (i32.const 1)    ;; fd (stdout)
      (i32.const 0)    ;; iov array pointer
      (i32.const 1)    ;; iov count
      (i32.const 20)   ;; pointer to store bytes written
    )
    drop
  )
)