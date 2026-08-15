# TODO

- The example prints garbage characters when reading the file using `BufReader.read_line()`. This is because `BufReader` builds strings using `core.str_from_ptr`, which currently relies on `quazi.str.from_ptr` intrinsic (mapped to `StrAsStr` opcode). The `StrAsStr` opcode only copies the pointer and leaves the length field uninitialized (garbage).
- `Array[str]` operations (like indexing and pushing) also suffer from truncating 16-byte fat pointers to 8 bytes due to `ArrayStore` and `ArrayLoad` opcodes only processing a single word. `array.qz` was updated to allocate 16 bytes per slot, but the compiler backend (`codegen.rs`/`encoder.rs`) still needs proper support for 2-word fat-pointer array access.
