# Option Type in Buff

Buff uses `Option<T>` (no null). Pattern match on `Some(value)` and `None`:

```buff
match result:
    case Some(v):
        print(v)
    case None:
        print("nothing")
```

## Your task

Implement `maybe_double` to double the inner value or return None.
