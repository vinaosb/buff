# Traits and Extension Methods

A **trait** declares a set of method signatures that a type can satisfy. Buff uses the brace form, with `;` after a bodyless signature marking a *required* method:

```buff
trait Summarize {
    func summary(self) -> String;
}
```

Buff does **not** have an `impl Trait for Type` syntax (the keyword `impl` is intentionally absent). Instead, methods are added to a type — primitive or user-defined — via an **`extend` block**:

```buff
extend String {
    func summary(self) -> String {
        return "String value"
    }
}

// Now you can call "hello".summary() and get "String value".
```

The trait and the extend block are separate declarations: the trait specifies the *contract*, the extend block provides the *implementation* on a specific type.

## Your task

1. Declare `trait Summarize { func summary(self) -> String; }` at the top.
2. Add `extend String { func summary(self) -> String { return "String value" } }` below it.
3. In `main`, call `"hello".summary()` and print the result.

**Hint:** traits use braces, methods inside traits end with `;` for required signatures. The extend body uses `func method(self) -> Ret { ... }` with the body in braces (NOT a colon+indent block — extend is brace-form).
