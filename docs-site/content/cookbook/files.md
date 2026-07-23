+++
title = "Files"
weight = 42
+++

# File I/O recipes

Recipes for reading, writing, walking, and inspecting the local
filesystem using the `Path`, `Dir`, and `Tempfile` prelude types
(T124j, v1.4 stdlib). All ops are panic-free — fallible paths return
`Option<T>` or `Result<T, Error>`.

## Read a text file

**Problem**: Read the entire contents of a file into a `String`.

**Solution**:

```buff
func read_config(path: String) -> Result<String, Error>:
    let p = Path.join(path)
    if not p.exists():
        return Error("config not found: " + path)
    return File.read(path)

func main():
    match read_config("config.toml") {
        Ok(text) => print(text),
        Err(e)   => print("read failed: " + e.string()),
    }
```

**Explanation**:

`Path.join(path)` constructs an owned path (the variadic `join(a, b,
...)` lowers to `PathBuf::from(a).join(b).join(c)...`). `p.exists()`
is a synchronous filesystem stat — `false` if the file is missing or
inaccessible, never a panic. `File.read(path)` is the prelude surface
for `std::fs::read_to_string`; it returns `Result<String, Error>` so
the `?`-propagation chain reads naturally.

`File` is the forward-looking surface for filesystem reads/writes
mirroring the shape from the language reference; `Path`, `Dir`, and
`Tempfile` are landed today (T124j). For bytes rather than text, use
`File.read_bytes(path)` (planned) or the `buff-archive` crate for
compressed formats.

## Write a text file

**Problem**: Persist a `String` to disk, overwriting any existing file.

**Solution**:

```buff
func save_log(path: String, content: String) -> Result<Void, Error>:
    return File.write(path, content)

func main():
    match save_log("app.log", "boot OK\n") {
        Ok(_)  => print("saved"),
        Err(e) => print("write failed: " + e.string()),
    }
```

**Explanation**:

`File.write(path, content)` is the prelude surface for
`std::fs::write`; it truncates the file if it exists and creates it
(plus parent directories, on supporting backends) if it doesn't. The
return type is `Result<Void, Error>` — `Void` because there's no
meaningful success value, just an absence of error.

For atomic writes (write to `.tmp`, then rename), use `Tempfile`
(see [Create a temporary file](#create-a-temporary-file) below) —
the rename is atomic on POSIX filesystems, so readers never see a
half-written file.

## Append to a file

**Problem**: Add a line to the end of an existing log file without
truncating it.

**Solution**:

```buff
func log_line(path: String, line: String) -> Result<Void, Error>:
    let entry = line + "\n"
    return File.append(path, entry)

func main():
    for i in 0..5:
        match log_line("events.log", "event " + i.string()) {
            Ok(_)  => print("ok"),
            Err(e) => print("append failed: " + e.string()),
        }
```

**Explanation**:

`File.append(path, data)` is the prelude surface for
`std::fs::OpenOptions::new().append(true).open(path)` followed by a
write — it opens the file (creating it if missing), seeks to
end-of-file, and writes. Each call is one syscall pair (open + write),
so appending in a tight loop benefits from a buffered writer.

For high-throughput logging, prefer the `Log` prelude module
(`Log.info(msg, ...)` → `tracing::info!`) — it batches writes,
supports structured fields, and integrates with `buff-observe` (T21)
for distributed tracing.

## Read a CSV file

**Problem**: Parse a CSV file into a `Vector<Vector<String>>`.

**Solution**:

```buff
func load_rows(path: String) -> Result<Vector<Vector<String>>, Error>:
    let text = File.read(path)?
    return Ok(Csv.parse(text))

func main():
    match load_rows("users.csv") {
        Ok(rows) =>
            for row in rows:
                print(row)
        Err(e) => print("csv load failed: " + e.string()),
    }
```

**Explanation**:

`Csv.parse(text)` is the prelude surface for the `csv` Rust crate
(T124i). It returns a `Vector<Vector<String>>` with no header
special-casing — every row, including the header, is a
`Vector<String>`. Call `rows[0]` to read the header explicitly.

For typed access, load into a `DataFrame` instead: see
[Load a CSV into a DataFrame](./dataframe/#load-a-csv-into-a-dataframe).
`DataFrame.from_csv(path)` infers column kinds (Int / Float / Bool /
String) at load time and exposes typed accessors per column.

## Walk a directory tree

**Problem**: Recursively list every file under a directory.

**Solution**:

```buff
func list_sources(root: String) -> Vector<Path>:
    return Dir.walk(root)

func main():
    let paths = list_sources("src")
    for p in paths:
        match p.extension():
            Some(ext) =>
                if ext == "buff":
                    print(p.basename())
            None => print("(no extension) " + p.basename())
```

**Explanation**:

`Dir.walk(path)` is the prelude surface for the `walkdir` Rust crate
(T124j). It returns a `Vector<Path>` of every file and directory
found during a depth-first traversal, skipping inaccessible entries
(never panics). For breadth-first or filtered traversal, post-process
the returned `Vector` — `Vector<T>.filter(...)`, `.map(...)`, and
`.reduce(...)` are the standard combinators.

`Path.extension()` returns `Option<String>` — `None` for files with
no extension. `Path.basename()` returns the trailing filename as a
`String` (empty when the path ends in `/` or `..`).

## Create a temporary file

**Problem**: Create a fresh, uniquely-named empty file in the OS temp
directory.

**Solution**:

```buff
func main():
    let tmp = Tempfile.create()
    File.write(tmp.string(), "scratch data")?
    print("wrote to " + tmp.string())
```

**Explanation**:

`Tempfile.create()` is the prelude surface for `tempfile::NamedTempFile`
(T124j). It returns a `Path` to a kept temp file — the underlying
`NamedTempFile` is dropped after the path is persisted, so the file
survives the call. The file lives in the OS-default temp directory
(`/tmp` on Linux, `%TEMP%` on Windows, `$TMPDIR` on macOS — surfaced
as `Tempfile.dir()`).

For atomic write-and-rename patterns, write to a `Tempfile`, then
`File.rename(tmp, final_path)`. The rename is atomic on POSIX, so
concurrent readers either see the old file or the new file — never a
partially-written one.
