    use super::*;
    use crate::ty::Type;

    #[test]
    fn prelude_type_lookup_known_names() {
        for &t in PreludeType::ALL {
            assert_eq!(prelude_type_lookup(t.name()), Some(t));
            assert!(is_prelude_type(t.name()));
            // Each DATETIME-FAMILY type's `buff_type()` round-trips
            // through the `is_prelude_datetime` predicate. Namespace-only
            // modules (T124c: `Log`) skip this check — they have no
            // value representation, so `buff_type()` returns `Void`
            // (which is correctly NOT a datetime).
            //
            // T124d: `Regex` is a runtime value but NOT a datetime, so
            // it also skips the `is_prelude_datetime` check (its
            // `buff_type()` returns `Type::Regex`, which round-trips
            // through `is_prelude_regex()` instead).
            //
            // T124h: `URL` is the second runtime-value-with-methods
            // type after Regex (T124d). Like Regex it's NOT a datetime,
            // so it skips the `is_prelude_datetime` check (its
            // `buff_type()` returns `Type::Url`, which round-trips
            // through `is_prelude_url()` instead).
            //
            // T124j: `Path` is the third runtime-value-with-methods
            // type after Regex (T124d) + URL (T124h). Like Regex +
            // URL it's NOT a datetime, so it skips the
            // `is_prelude_datetime` check (its `buff_type()` returns
            // `Type::Path`, which round-trips through
            // `is_prelude_path()` instead).
            //
            // T124l: `Process` is the fourth runtime-value-with-
            // methods type after Regex (T124d) + URL (T124h) + Path
            // (T124j). Like Regex + URL + Path it's NOT a datetime,
            // so it skips the `is_prelude_datetime` check (its
            // `buff_type()` returns `Type::Process`, which round-
            // trips through `is_prelude_process()` instead).
            if !t.is_namespace_only()
                && t != PreludeType::Regex
                && t != PreludeType::URL
                && t != PreludeType::Path
                && t != PreludeType::Process
                && t != PreludeType::Web
            {
                assert!(t.buff_type().is_prelude_datetime());
            }
        }
    }

    #[test]
    fn prelude_type_lookup_rejects_unknown() {
        assert!(!is_prelude_type("MyType"));
        assert!(!is_prelude_type(""));
        assert_eq!(prelude_type_lookup("DateTimeX"), None);
    }

    #[test]
    fn prelude_assoc_fn_lookup_valid_pairs() {
        assert_eq!(
            assoc_fn_lookup("DateTime", "now"),
            Some((PreludeType::DateTime, PreludeAssocFn::Now))
        );
        assert_eq!(
            assoc_fn_lookup("Duration", "days"),
            Some((PreludeType::Duration, PreludeAssocFn::Days))
        );
        assert_eq!(
            assoc_fn_lookup("Instant", "now"),
            Some((PreludeType::Instant, PreludeAssocFn::Now))
        );
    }

    #[test]
    fn prelude_assoc_fn_lookup_rejects_invalid_pairs() {
        // `days` is a Duration method — not DateTime.
        assert_eq!(assoc_fn_lookup("DateTime", "days"), None);
        // `now` is not a Duration method.
        assert_eq!(assoc_fn_lookup("Duration", "now"), None);
        // Unknown type.
        assert_eq!(assoc_fn_lookup("MyType", "now"), None);
        // Unknown method.
        assert_eq!(assoc_fn_lookup("DateTime", "unknown"), None);
    }

    // T124c: Log module — Log.<level>(msg, ...) assoc-fn lookups.
    #[test]
    fn prelude_log_assoc_fn_lookup_valid_pairs() {
        // All four Log levels resolve via the registry.
        assert_eq!(
            assoc_fn_lookup("Log", "debug"),
            Some((PreludeType::Log, PreludeAssocFn::Debug))
        );
        assert_eq!(
            assoc_fn_lookup("Log", "info"),
            Some((PreludeType::Log, PreludeAssocFn::Info))
        );
        assert_eq!(
            assoc_fn_lookup("Log", "warn"),
            Some((PreludeType::Log, PreludeAssocFn::Warn))
        );
        assert_eq!(
            assoc_fn_lookup("Log", "error"),
            Some((PreludeType::Log, PreludeAssocFn::Error))
        );
        // `Log` is recognised as a prelude type.
        assert!(is_prelude_type("Log"));
        // `Log.buff_type()` is `Void` (no runtime value).
        assert_eq!(PreludeType::Log.buff_type(), Type::Void);
        // `Log.is_namespace_only()` is true.
        assert!(PreludeType::Log.is_namespace_only());
        // The other prelude types are NOT namespace-only.
        assert!(!PreludeType::DateTime.is_namespace_only());
    }

    #[test]
    fn prelude_log_assoc_fn_lookup_rejects_invalid_pairs() {
        // Log.now is invalid (now is not a Log method).
        assert_eq!(assoc_fn_lookup("Log", "now"), None);
        // DateTime.info is invalid (info belongs to Log).
        assert_eq!(assoc_fn_lookup("DateTime", "info"), None);
        // Log.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Log", "unknown"), None);
    }

    #[test]
    fn prelude_log_assoc_fn_return_types() {
        // All four Log levels return Void.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Debug, &[]),
            Some(Type::Void)
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Info, &[]),
            Some(Type::Void)
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Warn, &[]),
            Some(Type::Void)
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Error, &[]),
            Some(Type::Void)
        );
        // Log + non-Log method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Now, &[]),
            None
        );
        // Non-Log type + Log method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Info, &[]),
            None
        );
    }

    #[test]
    fn prelude_assoc_fn_return_types() {
        // DateTime.now() -> DateTime
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Now, &[]),
            Some(Type::DateTime)
        );
        // DateTime.parse(s) -> DateTime
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::DateTime,
                PreludeAssocFn::Parse,
                &[Type::string()]
            ),
            Some(Type::DateTime)
        );
        // Duration.days(n) -> Duration
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Duration,
                PreludeAssocFn::Days,
                &[Type::int_default()]
            ),
            Some(Type::Duration)
        );
        // Instant.now() -> Instant
        assert_eq!(
            assoc_fn_return_type(PreludeType::Instant, PreludeAssocFn::Now, &[]),
            Some(Type::Instant)
        );
        // Invalid pair: DateTime.days(n) -> None
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::DateTime,
                PreludeAssocFn::Days,
                &[Type::int_default()]
            ),
            None
        );
    }

    #[test]
    fn prelude_instance_fn_return_types() {
        // dt.format(fmt) -> String
        assert_eq!(
            instance_fn_return_type(
                &Type::DateTime,
                PreludeInstanceFn::Format,
                &[Type::string()]
            ),
            Some(Type::string())
        );
        // dt.year() -> Int
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Year, &[]),
            Some(Type::int_default())
        );
        // dt.timestamp() -> Int
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Timestamp, &[]),
            Some(Type::int_default())
        );
        // date.format(fmt) -> String (Date also has format)
        assert_eq!(
            instance_fn_return_type(&Type::Date, PreludeInstanceFn::Format, &[Type::string()]),
            Some(Type::string())
        );
        // date.year() -> Int (Date has year, NOT hour)
        assert_eq!(
            instance_fn_return_type(&Type::Date, PreludeInstanceFn::Year, &[]),
            Some(Type::int_default())
        );
        // date.hour() -> None (Date has no hour component)
        assert_eq!(
            instance_fn_return_type(&Type::Date, PreludeInstanceFn::Hour, &[]),
            None
        );
        // Duration.format(...) -> None (Duration has no format method)
        assert_eq!(
            instance_fn_return_type(
                &Type::Duration,
                PreludeInstanceFn::Format,
                &[Type::string()]
            ),
            None
        );
        // Instant.format(...) -> None
        assert_eq!(
            instance_fn_return_type(&Type::Instant, PreludeInstanceFn::Format, &[Type::string()]),
            None
        );
    }

    #[test]
    fn prelude_instance_fn_lookup_dispatches_on_receiver_type() {
        // DateTime.format is valid.
        assert_eq!(
            instance_fn_lookup(&Type::DateTime, "format"),
            Some(PreludeInstanceFn::Format)
        );
        // Duration.format is NOT valid.
        assert_eq!(instance_fn_lookup(&Type::Duration, "format"), None);
        // Unknown method.
        assert_eq!(instance_fn_lookup(&Type::DateTime, "unknown"), None);
        // Non-prelude receiver (e.g. String).
        assert_eq!(instance_fn_lookup(&Type::String, "format"), None);
    }

    #[test]
    fn prelude_type_no_duplicates() {
        let names: Vec<&str> = PreludeType::ALL.iter().map(|t| t.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate prelude type names");
        // 5 datetime-family members shipped in T124b + 1 namespace module
        // (Log) shipped in T124c + 1 runtime-value-with-methods type
        // (Regex) shipped in T124d + 1 namespace-only module (Toml)
        // shipped in T124e + 3 namespace-only utility modules (Math,
        // Random, Strings) shipped in T124f + 2 namespace-only system
        // modules (Args, Env) shipped in T124g + 4 namespace-only web
        // modules (Base64, Hex, URLEncode, UUID) + 1 runtime-value-
        // with-methods type (URL) shipped in T124h + 2 namespace-only
        // data-format modules (Yaml, Csv) shipped in T124i + 1 runtime-
        // value-with-methods type (Path) + 2 namespace-only modules
        // (Dir, Tempfile) shipped in T124j + 2 namespace-only crypto
        // modules (Hash, HMAC) shipped in T124k + 1 namespace-only
        // system-introspection module (OS) + 1 runtime-value-with-
        // methods type (Process) shipped in T124l + 3 namespace-only
        // networking modules (TCP, UDP, WebSocket) shipped in T124m
        // + 1 namespace-only module (Channel) shipped in T2 + 1
        // forward-declaration-only namespace (Tensor) + 1 runtime-
        // value-with-methods type (Image) shipped in T9 + 2
        // namespace-only modules (Signal, Window) + 1 runtime-value-
        // with-methods type (Spectrum) shipped in T11 + 1 runtime-
        // value-with-methods type (DataFrame) shipped in T7
        // = 36 total prelude types.
        assert_eq!(PreludeType::ALL.len(), 37);
    }

    #[test]
    fn prelude_assoc_fn_no_duplicates() {
        let names: Vec<&str> = PreludeAssocFn::ALL.iter().map(|f| f.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate assoc-fn names");
    }

    #[test]
    fn prelude_instance_fn_no_duplicates() {
        let names: Vec<&str> = PreludeInstanceFn::ALL.iter().map(|f| f.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate instance-fn names");
    }

    #[test]
    fn buff_type_constructors_and_predicate() {
        // Each Type constructor + the is_prelude_datetime predicate.
        assert!(Type::datetime().is_prelude_datetime());
        assert!(Type::date().is_prelude_datetime());
        assert!(Type::time().is_prelude_datetime());
        assert!(Type::duration().is_prelude_datetime());
        assert!(Type::instant().is_prelude_datetime());
        // Non-datetime types are not flagged.
        assert!(!Type::int_default().is_prelude_datetime());
        assert!(!Type::string().is_prelude_datetime());
        assert!(!Type::Unknown.is_prelude_datetime());
        // T124d: Regex type + predicate. Regex is NOT a datetime family
        // member — its dedicated `is_prelude_regex` predicate captures
        // the runtime-value-but-not-datetime case.
        assert!(Type::regex().is_prelude_regex());
        assert!(!Type::regex().is_prelude_datetime());
        assert!(!Type::DateTime.is_prelude_regex());
        assert!(!Type::string().is_prelude_regex());
        // Cross-check via the prelude-type registry: `Regex.buff_type()`
        // round-trips through `is_prelude_regex` (the only prelude type
        // for which it does).
        assert!(PreludeType::Regex.buff_type().is_prelude_regex());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_regex());
        assert!(!PreludeType::Log.buff_type().is_prelude_regex());
        // T124h: URL type + predicate. URL is NOT a datetime family
        // member and NOT a Regex - its dedicated `is_prelude_url`
        // predicate captures the parsed-URL runtime-value case.
        assert!(Type::url().is_prelude_url());
        assert!(!Type::url().is_prelude_datetime());
        assert!(!Type::url().is_prelude_regex());
        assert!(!Type::DateTime.is_prelude_url());
        assert!(!Type::Regex.is_prelude_url());
        assert!(!Type::string().is_prelude_url());
        // Cross-check via the prelude-type registry: `URL.buff_type()`
        // round-trips through `is_prelude_url` (the only prelude type
        // for which it does). The namespace-only web modules (Base64 /
        // Hex / URLEncode / UUID) do NOT round-trip (their `buff_type()`
        // returns `Type::Void`).
        assert!(PreludeType::URL.buff_type().is_prelude_url());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_url());
        assert!(!PreludeType::Regex.buff_type().is_prelude_url());
        assert!(!PreludeType::Log.buff_type().is_prelude_url());
        assert!(!PreludeType::Base64.buff_type().is_prelude_url());
        assert!(!PreludeType::UUID.buff_type().is_prelude_url());
        // T124j: Path type + predicate. Path is NOT a datetime family
        // member, NOT a Regex, and NOT a URL - its dedicated
        // `is_prelude_path` predicate captures the filesystem-path
        // runtime-value case.
        assert!(Type::path().is_prelude_path());
        assert!(!Type::path().is_prelude_datetime());
        assert!(!Type::path().is_prelude_regex());
        assert!(!Type::path().is_prelude_url());
        assert!(!Type::DateTime.is_prelude_path());
        assert!(!Type::Regex.is_prelude_path());
        assert!(!Type::Url.is_prelude_path());
        assert!(!Type::string().is_prelude_path());
        // Cross-check via the prelude-type registry: `Path.buff_type()`
        // round-trips through `is_prelude_path` (the only prelude type
        // for which it does). The namespace-only Dir / Tempfile modules
        // do NOT round-trip (their `buff_type()` returns `Type::Void`).
        assert!(PreludeType::Path.buff_type().is_prelude_path());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_path());
        assert!(!PreludeType::Regex.buff_type().is_prelude_path());
        assert!(!PreludeType::URL.buff_type().is_prelude_path());
        assert!(!PreludeType::Log.buff_type().is_prelude_path());
        assert!(!PreludeType::Dir.buff_type().is_prelude_path());
        assert!(!PreludeType::Tempfile.buff_type().is_prelude_path());
        // T124l: Process type + predicate. Process is NOT a datetime
        // family member, NOT a Regex, NOT a URL, and NOT a Path -
        // its dedicated `is_prelude_process` predicate captures
        // the spawned-process runtime-value case.
        assert!(Type::process().is_prelude_process());
        assert!(!Type::process().is_prelude_datetime());
        assert!(!Type::process().is_prelude_regex());
        assert!(!Type::process().is_prelude_url());
        assert!(!Type::process().is_prelude_path());
        assert!(!Type::DateTime.is_prelude_process());
        assert!(!Type::Regex.is_prelude_process());
        assert!(!Type::Url.is_prelude_process());
        assert!(!Type::Path.is_prelude_process());
        assert!(!Type::string().is_prelude_process());
        // Cross-check via the prelude-type registry: `Process
        // .buff_type()` round-trips through `is_prelude_process`
        // (the only prelude type for which it does). The
        // namespace-only OS module does NOT round-trip (its
        // `buff_type()` returns `Type::Void`).
        assert!(PreludeType::Process.buff_type().is_prelude_process());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_process());
        assert!(!PreludeType::Regex.buff_type().is_prelude_process());
        assert!(!PreludeType::URL.buff_type().is_prelude_process());
        assert!(!PreludeType::Path.buff_type().is_prelude_process());
        assert!(!PreludeType::Log.buff_type().is_prelude_process());
        assert!(!PreludeType::OS.buff_type().is_prelude_process());
    }

    #[test]
    fn type_display_datetime_family() {
        assert_eq!(Type::DateTime.to_string(), "DateTime");
        assert_eq!(Type::Date.to_string(), "Date");
        assert_eq!(Type::Time.to_string(), "Time");
        assert_eq!(Type::Duration.to_string(), "Duration");
        assert_eq!(Type::Instant.to_string(), "Instant");
        // T124d: Regex Display mirrors the Buff surface name.
        assert_eq!(Type::Regex.to_string(), "Regex");
        // T124h: URL Display mirrors the Buff surface name (all-caps,
        // matches the `URL.parse(...)` user-facing spelling).
        assert_eq!(Type::Url.to_string(), "URL");
        // T124j: Path Display mirrors the Buff surface name.
        assert_eq!(Type::Path.to_string(), "Path");
        // T124l: Process Display mirrors the Buff surface name.
        assert_eq!(Type::Process.to_string(), "Process");
    }

    // T124d: Regex module — `Regex.compile(p)` assoc-fn lookups + return type.
    #[test]
    fn prelude_regex_assoc_fn_lookup_valid_pairs() {
        // `Regex.compile` is the single associated function on the Regex
        // prelude type. It returns a real `Regex` value (NOT Void like
        // Log's namespace-only assoc fns).
        assert_eq!(
            assoc_fn_lookup("Regex", "compile"),
            Some((PreludeType::Regex, PreludeAssocFn::Compile))
        );
        // `Regex` is recognised as a prelude type.
        assert!(is_prelude_type("Regex"));
        // `Regex.buff_type()` is `Regex` (a real runtime value, NOT Void).
        assert_eq!(PreludeType::Regex.buff_type(), Type::Regex);
        // `Regex.is_namespace_only()` is false (it IS a runtime value).
        assert!(!PreludeType::Regex.is_namespace_only());
        // The other prelude types are NOT Regex (round-trip via buff_type).
        assert!(!PreludeType::DateTime.buff_type().is_prelude_regex());
        assert!(PreludeType::Regex.buff_type().is_prelude_regex());
    }

    #[test]
    fn prelude_regex_assoc_fn_lookup_rejects_invalid_pairs() {
        // Regex.now is invalid (now is not a Regex method).
        assert_eq!(assoc_fn_lookup("Regex", "now"), None);
        // DateTime.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("DateTime", "compile"), None);
        // Regex.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Regex", "unknown"), None);
        // Regex.parse is invalid (Regex has compile, not parse).
        assert_eq!(assoc_fn_lookup("Regex", "parse"), None);
    }

    #[test]
    fn prelude_regex_assoc_fn_return_type() {
        // Regex.compile(pattern) -> Regex.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Regex,
                PreludeAssocFn::Compile,
                &[Type::string()]
            ),
            Some(Type::Regex)
        );
        // Regex + non-Regex method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Now, &[]),
            None
        );
        // Non-Regex type + Regex method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Compile, &[]),
            None
        );
        // Log + Regex.compile is invalid (Log is namespace-only).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Compile, &[]),
            None
        );
    }

    #[test]
    fn prelude_regex_instance_fn_lookup_valid_pairs() {
        // All four Regex instance methods resolve via the registry when
        // the receiver is `Type::Regex`.
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "match"),
            Some(PreludeInstanceFn::Match)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "find"),
            Some(PreludeInstanceFn::Find)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "replace"),
            Some(PreludeInstanceFn::Replace)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "captures"),
            Some(PreludeInstanceFn::Captures)
        );
    }

    #[test]
    fn prelude_regex_instance_fn_lookup_rejects_invalid_pairs() {
        // Regex.format is invalid (format belongs to DateTime/Date/Time).
        assert_eq!(instance_fn_lookup(&Type::Regex, "format"), None);
        // Regex.year is invalid.
        assert_eq!(instance_fn_lookup(&Type::Regex, "year"), None);
        // Regex.unknown is invalid.
        assert_eq!(instance_fn_lookup(&Type::Regex, "unknown"), None);
        // Regex.match is invalid when the receiver is NOT Regex.
        assert_eq!(instance_fn_lookup(&Type::DateTime, "match"), None);
        assert_eq!(instance_fn_lookup(&Type::String, "find"), None);
    }

    #[test]
    fn prelude_regex_instance_fn_return_types() {
        // regex.match(text) -> Option<String>.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Match, &[Type::string()]),
            Some(Type::option(Type::string()))
        );
        // regex.find(text) -> Option<String>.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Find, &[Type::string()]),
            Some(Type::option(Type::string()))
        );
        // regex.replace(text, repl) -> String.
        assert_eq!(
            instance_fn_return_type(
                &Type::Regex,
                PreludeInstanceFn::Replace,
                &[Type::string(), Type::string()]
            ),
            Some(Type::string())
        );
        // regex.captures(text) -> Map<String, String>.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Captures, &[Type::string()]),
            Some(Type::map(Type::string(), Type::string()))
        );
        // Non-Regex receiver + Regex method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Match, &[Type::string()]),
            None
        );
        // Regex receiver + non-Regex method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Format, &[Type::string()]),
            None
        );
    }

    // T124e: Toml module — `Toml.parse(s)` / `Toml.stringify(v)` assoc-fn
    // lookups + return types. Mirrors the Log namespace-only precedent
    // (T124c) but with non-Void return types (Map / String).
    #[test]
    fn prelude_toml_assoc_fn_lookup_valid_pairs() {
        // `Toml.parse` reuses the registry's shared `Parse` variant
        // (also used by DateTime.parse / Date.parse).
        assert_eq!(
            assoc_fn_lookup("Toml", "parse"),
            Some((PreludeType::Toml, PreludeAssocFn::Parse))
        );
        // `Toml.stringify` is the dedicated Toml-only assoc fn.
        assert_eq!(
            assoc_fn_lookup("Toml", "stringify"),
            Some((PreludeType::Toml, PreludeAssocFn::Stringify))
        );
        // `Toml` is recognised as a prelude type.
        assert!(is_prelude_type("Toml"));
        // `Toml.buff_type()` is `Void` (no runtime value — namespace-only
        // like Log).
        assert_eq!(PreludeType::Toml.buff_type(), Type::Void);
        // `Toml.is_namespace_only()` is true.
        assert!(PreludeType::Toml.is_namespace_only());
        // The datetime-family types are NOT namespace-only.
        assert!(!PreludeType::DateTime.is_namespace_only());
        // Regex is NOT namespace-only (it's a real runtime value).
        assert!(!PreludeType::Regex.is_namespace_only());
    }

    #[test]
    fn prelude_toml_assoc_fn_lookup_rejects_invalid_pairs() {
        // Toml.now is invalid (now is not a Toml method).
        assert_eq!(assoc_fn_lookup("Toml", "now"), None);
        // Toml.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Toml", "compile"), None);
        // Toml.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Toml", "unknown"), None);
        // Toml.debug is invalid (debug belongs to Log).
        assert_eq!(assoc_fn_lookup("Toml", "debug"), None);
        // DateTime.stringify is invalid (stringify belongs to Toml).
        assert_eq!(assoc_fn_lookup("DateTime", "stringify"), None);
        // Regex.stringify is invalid.
        assert_eq!(assoc_fn_lookup("Regex", "stringify"), None);
        // Log.stringify is invalid (Log is namespace-only).
        assert_eq!(assoc_fn_lookup("Log", "stringify"), None);
    }

    #[test]
    fn prelude_toml_assoc_fn_return_types() {
        // Toml.parse(s) -> Map<String, Unknown>. The value type is
        // Unknown because TOML values are heterogeneous (scalars /
        // arrays / sub-tables); the codegen turbofish-es to the
        // concrete `HashMap<String, toml::Value>` at the Rust level.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Parse, &[Type::string()]),
            Some(Type::map(Type::string(), Type::Unknown))
        );
        // Toml.stringify(v) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Toml,
                PreludeAssocFn::Stringify,
                &[Type::map(Type::string(), Type::Unknown)]
            ),
            Some(Type::string())
        );
        // Toml + non-Toml method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Compile, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Debug, &[]),
            None
        );
        // Non-Toml type + Toml method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Stringify, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Stringify, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Stringify, &[]),
            None
        );
    }

    #[test]
    fn prelude_toml_namespace_only_predicate() {
        // Both Log and Toml are namespace-only modules.
        assert!(PreludeType::Log.is_namespace_only());
        assert!(PreludeType::Toml.is_namespace_only());
        // T124f: Math / Random / Strings are also namespace-only.
        assert!(PreludeType::Math.is_namespace_only());
        assert!(PreludeType::Random.is_namespace_only());
        assert!(PreludeType::Strings.is_namespace_only());
        // The datetime family + Regex are NOT namespace-only.
        assert!(!PreludeType::DateTime.is_namespace_only());
        assert!(!PreludeType::Date.is_namespace_only());
        assert!(!PreludeType::Time.is_namespace_only());
        assert!(!PreludeType::Duration.is_namespace_only());
        assert!(!PreludeType::Instant.is_namespace_only());
        assert!(!PreludeType::Regex.is_namespace_only());
        // T124g: Args / Env are also namespace-only modules (mirror
        // Log / Toml / Math / Random / Strings). Both wrap `std::env`
        // and have NO runtime value representation.
        assert!(PreludeType::Args.is_namespace_only());
        assert!(PreludeType::Env.is_namespace_only());
        // T124h: Base64 / Hex / URLEncode / UUID are also namespace-only
        // modules (mirror Log / Toml / Math / Random / Strings / Args /
        // Env). All four wrap a Rust crate (base64 / hex /
        // percent-encoding / uuid) and have NO runtime value
        // representation (UUIDs surface as their canonical String form).
        assert!(PreludeType::Base64.is_namespace_only());
        assert!(PreludeType::Hex.is_namespace_only());
        assert!(PreludeType::URLEncode.is_namespace_only());
        assert!(PreludeType::UUID.is_namespace_only());
        // T124h: URL is NOT namespace-only - it's a real runtime value
        // type (mirrors Regex's runtime-value-with-rich-instance-methods
        // stance from T124d). Distinct from the namespace-only Base64 /
        // Hex / URLEncode / UUID modules it shipped alongside.
        assert!(!PreludeType::URL.is_namespace_only());
        // T124i: Yaml / Csv are also namespace-only modules (mirror
        // Log / Toml / Math / Random / Strings / Args / Env / Base64 /
        // Hex / URLEncode / UUID). Both wrap a Rust crate (serde_yml
        // / csv) and have NO runtime value representation.
        assert!(PreludeType::Yaml.is_namespace_only());
        assert!(PreludeType::Csv.is_namespace_only());
        // T124i: The count of namespace-only modules is now exactly 13
        // (Log + Toml + Math + Random + Strings + Args + Env + Base64 +
        // Hex + URLEncode + UUID + Yaml + Csv). URL is NOT in this count
        // (it's a runtime value type, not a namespace module).
        // T124j: Dir + Tempfile are also namespace-only modules
        // (mirror Yaml / Csv / Log / ...). Path is NOT in this
        // count (it's a runtime-value-with-instance-methods type,
        // mirrors Regex T124d + URL T124h). The count is now
        // exactly 15 (Log + Toml + Math + Random + Strings + Args +
        // Env + Base64 + Hex + URLEncode + UUID + Yaml + Csv + Dir
        // + Tempfile).
        assert!(PreludeType::Dir.is_namespace_only());
        assert!(PreludeType::Tempfile.is_namespace_only());
        // T124j: Path is NOT namespace-only - it's a real runtime
        // value type (mirrors Regex T124d + URL T124h's runtime-
        // value-with-rich-instance-methods stance). Distinct from
        // the namespace-only Dir / Tempfile modules it shipped
        // alongside.
        assert!(!PreludeType::Path.is_namespace_only());
        // T124k: Hash / HMAC are also namespace-only modules
        // (mirror Log / Toml / Math / Random / Strings / Args /
        // Env / Base64 / Hex / URLEncode / UUID / Yaml / Csv /
        // Dir / Tempfile). Both wrap a Rust crate (sha2 + md5 +
        // hmac) and have NO runtime value representation (every
        // call returns a hex String - the digest / MAC).
        assert!(PreludeType::Hash.is_namespace_only());
        assert!(PreludeType::HMAC.is_namespace_only());
        // T124l: OS is also a namespace-only module (mirror
        // Log / Toml / Math / Random / Strings / Args / Env /
        // Base64 / Hex / URLEncode / UUID / Yaml / Csv / Dir /
        // Tempfile / Hash / HMAC). It wraps `std::env::consts`
        // + env-var hostname + `num_cpus` and has NO runtime
        // value representation (every call returns a String /
        // Int - the OS name / arch / hostname / cpu count).
        assert!(PreludeType::OS.is_namespace_only());
        // T124l: Process is NOT namespace-only - it's a real
        // runtime value type (mirrors Regex T124d + URL T124h
        // + Path T124j's runtime-value-with-rich-instance-
        // methods stance). Distinct from the namespace-only OS
        // module it shipped alongside.
        assert!(!PreludeType::Process.is_namespace_only());
        // T124m: TCP / UDP / WebSocket are also namespace-only
        // modules (mirror Log / Toml / OS / Process's namespace
        // counterpart). The runtime-value types they construct
        // (Connection / Socket / WsConnection) are separate Type
        // variants (NOT namespace-only themselves - they're real
        // runtime values, like Regex / URL / Path / Process).
        assert!(PreludeType::TCP.is_namespace_only());
        assert!(PreludeType::UDP.is_namespace_only());
        assert!(PreludeType::WebSocket.is_namespace_only());
        let namespace_only_count = PreludeType::ALL
            .iter()
            .filter(|t| t.is_namespace_only())
            .count();
        // T124m: bumped from 18 to 21 (TCP + UDP + WebSocket
        // namespace-only; Connection / Socket / WsConnection are
        // NOT namespace-only themselves - they're runtime-value
        // types constructed by the assoc fns).
        assert_eq!(namespace_only_count, 21);
    }

    // T124f: Math module - `Math.<fn>(x, ...)` assoc-fn lookups +
    // return types + the associated-constant mechanism for `Math.PI` /
    // `Math.E`. Mirrors the Log / Toml namespace-only precedent (T124c
    // / T124e) but with Float return types + the first associated
    // constants in the registry.
    #[test]
    fn prelude_math_assoc_fn_lookup_valid_pairs() {
        // All 11 Math assoc fns resolve via the registry.
        assert_eq!(
            assoc_fn_lookup("Math", "sqrt"),
            Some((PreludeType::Math, PreludeAssocFn::Sqrt))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "sin"),
            Some((PreludeType::Math, PreludeAssocFn::Sin))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "cos"),
            Some((PreludeType::Math, PreludeAssocFn::Cos))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "tan"),
            Some((PreludeType::Math, PreludeAssocFn::Tan))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "abs"),
            Some((PreludeType::Math, PreludeAssocFn::Abs))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "floor"),
            Some((PreludeType::Math, PreludeAssocFn::Floor))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "ceil"),
            Some((PreludeType::Math, PreludeAssocFn::Ceil))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "round"),
            Some((PreludeType::Math, PreludeAssocFn::Round))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "pow"),
            Some((PreludeType::Math, PreludeAssocFn::Pow))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "min"),
            Some((PreludeType::Math, PreludeAssocFn::Min))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "max"),
            Some((PreludeType::Math, PreludeAssocFn::Max))
        );
        // `Math` is recognised as a prelude type.
        assert!(is_prelude_type("Math"));
        // `Math.buff_type()` is `Void` (no runtime value - namespace-only
        // like Log / Toml).
        assert_eq!(PreludeType::Math.buff_type(), Type::Void);
        // `Math.is_namespace_only()` is true.
        assert!(PreludeType::Math.is_namespace_only());
    }

    #[test]
    fn prelude_math_assoc_fn_lookup_rejects_invalid_pairs() {
        // Math.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Math", "now"), None);
        // Math.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Math", "compile"), None);
        // Math.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Math", "unknown"), None);
        // Math.debug is invalid (debug belongs to Log).
        assert_eq!(assoc_fn_lookup("Math", "debug"), None);
        // Math.parse is invalid (Math has no parse method).
        assert_eq!(assoc_fn_lookup("Math", "parse"), None);
        // DateTime.sqrt is invalid (sqrt belongs to Math).
        assert_eq!(assoc_fn_lookup("DateTime", "sqrt"), None);
        // Log.sin is invalid (Log is namespace-only).
        assert_eq!(assoc_fn_lookup("Log", "sin"), None);
    }

    #[test]
    fn prelude_math_assoc_fn_return_types() {
        // All Math methods return Float (f64 width).
        let expected = Some(Type::float_default());
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Sqrt,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Sin,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Cos,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Tan,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Abs,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Floor,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Ceil,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Round,
                &[Type::float_default()]
            ),
            expected
        );
        // pow takes 2 args, but still returns Float.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Pow,
                &[Type::float_default(), Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Min,
                &[Type::float_default(), Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Max,
                &[Type::float_default(), Type::float_default()]
            ),
            expected
        );
        // Math + non-Math method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Math, PreludeAssocFn::Now, &[]),
            None
        );
        // Non-Math type + Math method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Sqrt, &[]),
            None
        );
    }

    // T124f: Math associated constants - the FIRST associated-constant
    // prelude mechanism. `Math.PI` / `Math.E` resolve via the dedicated
    // `assoc_const_lookup` registry (separate from assoc fns because
    // the parser produces a zero-arg MethodCall that the codegen must
    // rewrite to the Rust `std::f64::consts::PI` / `E` path rather
    // than the literal field access `Math.PI`).
    #[test]
    fn prelude_math_assoc_const_lookup_valid_pairs() {
        assert_eq!(
            assoc_const_lookup("Math", "PI"),
            Some((PreludeType::Math, PreludeAssocConst::Pi))
        );
        assert_eq!(
            assoc_const_lookup("Math", "E"),
            Some((PreludeType::Math, PreludeAssocConst::E))
        );
    }

    #[test]
    fn prelude_math_assoc_const_lookup_rejects_invalid_pairs() {
        // Math.TAU is invalid (not in the T124f surface).
        assert_eq!(assoc_const_lookup("Math", "TAU"), None);
        // Math.PHI is invalid.
        assert_eq!(assoc_const_lookup("Math", "PHI"), None);
        // Math.pi (lowercase) is invalid (constants are UPPERCASE).
        assert_eq!(assoc_const_lookup("Math", "pi"), None);
        // Math.sqrt is not a constant.
        assert_eq!(assoc_const_lookup("Math", "sqrt"), None);
        // DateTime.PI is invalid (PI belongs to Math).
        assert_eq!(assoc_const_lookup("DateTime", "PI"), None);
        // Log.PI is invalid (Log is namespace-only with no constants).
        assert_eq!(assoc_const_lookup("Log", "PI"), None);
        // Toml.E is invalid.
        assert_eq!(assoc_const_lookup("Toml", "E"), None);
    }

    #[test]
    fn prelude_math_assoc_const_return_types() {
        // Math.PI / Math.E -> Float (f64).
        assert_eq!(
            assoc_const_return_type(PreludeType::Math, PreludeAssocConst::Pi),
            Some(Type::float_default())
        );
        assert_eq!(
            assoc_const_return_type(PreludeType::Math, PreludeAssocConst::E),
            Some(Type::float_default())
        );
        // Non-Math type + Math const is invalid.
        assert_eq!(
            assoc_const_return_type(PreludeType::DateTime, PreludeAssocConst::Pi),
            None
        );
        assert_eq!(
            assoc_const_return_type(PreludeType::Log, PreludeAssocConst::E),
            None
        );
    }

    #[test]
    fn prelude_assoc_const_all_and_no_duplicates() {
        // 2 associated constants: PI + E.
        assert_eq!(PreludeAssocConst::ALL.len(), 2);
        let names: Vec<&str> = PreludeAssocConst::ALL.iter().map(|c| c.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate assoc-const names");
        // Names are UPPERCASE per Rust / Buff const convention.
        assert_eq!(PreludeAssocConst::Pi.name(), "PI");
        assert_eq!(PreludeAssocConst::E.name(), "E");
    }

    // T124f: Random module - `Random.<fn>(...)` assoc-fn lookups +
    // return types. Mirrors the Math / Log namespace-only precedent
    // but with mixed return types (Int / Float / Option / Vector).
    #[test]
    fn prelude_random_assoc_fn_lookup_valid_pairs() {
        assert_eq!(
            assoc_fn_lookup("Random", "int"),
            Some((PreludeType::Random, PreludeAssocFn::Int))
        );
        assert_eq!(
            assoc_fn_lookup("Random", "float"),
            Some((PreludeType::Random, PreludeAssocFn::Float))
        );
        assert_eq!(
            assoc_fn_lookup("Random", "choice"),
            Some((PreludeType::Random, PreludeAssocFn::Choice))
        );
        assert_eq!(
            assoc_fn_lookup("Random", "shuffle"),
            Some((PreludeType::Random, PreludeAssocFn::Shuffle))
        );
        // `Random` is recognised as a prelude type.
        assert!(is_prelude_type("Random"));
        // `Random.buff_type()` is `Void` (namespace-only).
        assert_eq!(PreludeType::Random.buff_type(), Type::Void);
        // `Random.is_namespace_only()` is true.
        assert!(PreludeType::Random.is_namespace_only());
    }

    #[test]
    fn prelude_random_assoc_fn_lookup_rejects_invalid_pairs() {
        // Random.now is invalid.
        assert_eq!(assoc_fn_lookup("Random", "now"), None);
        // Random.compile is invalid.
        assert_eq!(assoc_fn_lookup("Random", "compile"), None);
        // Random.sqrt is invalid (sqrt belongs to Math).
        assert_eq!(assoc_fn_lookup("Random", "sqrt"), None);
        // Math.int is invalid (int belongs to Random).
        assert_eq!(assoc_fn_lookup("Math", "int"), None);
    }

    #[test]
    fn prelude_random_assoc_fn_return_types() {
        // Random.int(min, max) -> Int<64>.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Random,
                PreludeAssocFn::Int,
                &[Type::int_default(), Type::int_default()]
            ),
            Some(Type::int_default())
        );
        // Random.float() -> Float.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Random, PreludeAssocFn::Float, &[]),
            Some(Type::float_default())
        );
        // Random.choice(vec) -> Option<Unknown> (element type inferred
        // by Rust at the use site; Unknown at the registry level).
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Random,
                PreludeAssocFn::Choice,
                &[Type::vector(Type::Unknown)]
            ),
            Some(Type::option(Type::Unknown))
        );
        // Random.shuffle(vec) -> Vector<Unknown>.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Random,
                PreludeAssocFn::Shuffle,
                &[Type::vector(Type::Unknown)]
            ),
            Some(Type::vector(Type::Unknown))
        );
    }

    // T124f: Strings module - `Strings.<fn>(...)` assoc-fn lookups +
    // return types. Mirrors the Math / Log namespace-only precedent.
    #[test]
    fn prelude_strings_assoc_fn_lookup_valid_pairs() {
        assert_eq!(
            assoc_fn_lookup("Strings", "split"),
            Some((PreludeType::Strings, PreludeAssocFn::Split))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "join"),
            Some((PreludeType::Strings, PreludeAssocFn::Join))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "trim"),
            Some((PreludeType::Strings, PreludeAssocFn::Trim))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "replace"),
            Some((PreludeType::Strings, PreludeAssocFn::Replace))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "contains"),
            Some((PreludeType::Strings, PreludeAssocFn::Contains))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "starts_with"),
            Some((PreludeType::Strings, PreludeAssocFn::StartsWith))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "to_uppercase"),
            Some((PreludeType::Strings, PreludeAssocFn::ToUppercase))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "to_lowercase"),
            Some((PreludeType::Strings, PreludeAssocFn::ToLowercase))
        );
        // `Strings` is recognised as a prelude type.
        assert!(is_prelude_type("Strings"));
        // `Strings.buff_type()` is `Void` (namespace-only).
        assert_eq!(PreludeType::Strings.buff_type(), Type::Void);
        // `Strings.is_namespace_only()` is true.
        assert!(PreludeType::Strings.is_namespace_only());
    }

    #[test]
    fn prelude_strings_assoc_fn_lookup_rejects_invalid_pairs() {
        // Strings.now is invalid.
        assert_eq!(assoc_fn_lookup("Strings", "now"), None);
        // Strings.compile is invalid.
        assert_eq!(assoc_fn_lookup("Strings", "compile"), None);
        // Strings.sqrt is invalid (sqrt belongs to Math).
        assert_eq!(assoc_fn_lookup("Strings", "sqrt"), None);
        // Strings.int is invalid (int belongs to Random).
        assert_eq!(assoc_fn_lookup("Strings", "int"), None);
        // Math.split is invalid (split belongs to Strings).
        assert_eq!(assoc_fn_lookup("Math", "split"), None);
    }

    #[test]
    fn prelude_strings_assoc_fn_return_types() {
        // Strings.split(text, sep) -> Vector<String>.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Split,
                &[Type::string(), Type::string()]
            ),
            Some(Type::vector(Type::string()))
        );
        // Strings.join(vec, sep) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Join,
                &[Type::vector(Type::string()), Type::string()]
            ),
            Some(Type::string())
        );
        // Strings.trim(text) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Trim,
                &[Type::string()]
            ),
            Some(Type::string())
        );
        // Strings.replace(text, from, to) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Replace,
                &[Type::string(), Type::string(), Type::string()]
            ),
            Some(Type::string())
        );
        // Strings.contains(text, substr) -> Bool.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Contains,
                &[Type::string(), Type::string()]
            ),
            Some(Type::bool())
        );
        // Strings.starts_with(text, prefix) -> Bool.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::StartsWith,
                &[Type::string(), Type::string()]
            ),
            Some(Type::bool())
        );
        // Strings.to_uppercase(text) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::ToUppercase,
                &[Type::string()]
            ),
            Some(Type::string())
        );
        // Strings.to_lowercase(text) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::ToLowercase,
                &[Type::string()]
            ),
            Some(Type::string())
        );
    }

    // T124i: Yaml module - `Yaml.parse(s)` / `Yaml.stringify(v)` assoc-fn
    // lookups + return types. Mirrors the Toml namespace-only precedent
    // (T124e) but for the `serde_yml` Rust crate (the maintained fork
    // of the deprecated `serde_yaml`).
    #[test]
    fn prelude_yaml_assoc_fn_lookup_valid_pairs() {
        // `Yaml.parse` reuses the registry's shared `Parse` variant
        // (also used by DateTime.parse / Date.parse / Toml.parse /
        // URL.parse / UUID.parse).
        assert_eq!(
            assoc_fn_lookup("Yaml", "parse"),
            Some((PreludeType::Yaml, PreludeAssocFn::Parse))
        );
        // `Yaml.stringify` reuses the registry's shared `Stringify`
        // variant (also used by Toml.stringify).
        assert_eq!(
            assoc_fn_lookup("Yaml", "stringify"),
            Some((PreludeType::Yaml, PreludeAssocFn::Stringify))
        );
        // `Yaml` is recognised as a prelude type.
        assert!(is_prelude_type("Yaml"));
        // `Yaml.buff_type()` is `Void` (no runtime value - namespace-only
        // like Toml / Log).
        assert_eq!(PreludeType::Yaml.buff_type(), Type::Void);
        // `Yaml.is_namespace_only()` is true.
        assert!(PreludeType::Yaml.is_namespace_only());
    }

    #[test]
    fn prelude_yaml_assoc_fn_lookup_rejects_invalid_pairs() {
        // Yaml.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Yaml", "now"), None);
        // Yaml.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Yaml", "compile"), None);
        // Yaml.debug is invalid (debug belongs to Log).
        assert_eq!(assoc_fn_lookup("Yaml", "debug"), None);
        // Yaml.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Yaml", "unknown"), None);
        // DateTime.stringify is invalid (stringify belongs to Toml/Yaml/Csv).
        assert_eq!(assoc_fn_lookup("DateTime", "stringify"), None);
        // Regex.stringify is invalid.
        assert_eq!(assoc_fn_lookup("Regex", "stringify"), None);
        // Log.stringify is invalid (Log is namespace-only with debug/info/...).
        assert_eq!(assoc_fn_lookup("Log", "stringify"), None);
    }

    #[test]
    fn prelude_yaml_assoc_fn_return_types() {
        // Yaml.parse(s) -> Map<String, Unknown>. The value type is
        // Unknown because YAML values are heterogeneous (scalars /
        // arrays / sub-mappings); the codegen turbofish-es to the
        // concrete `HashMap<String, serde_yml::Value>` at the Rust
        // level. Mirrors Toml.parse exactly.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Parse, &[Type::string()]),
            Some(Type::map(Type::string(), Type::Unknown))
        );
        // Yaml.stringify(v) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Yaml,
                PreludeAssocFn::Stringify,
                &[Type::map(Type::string(), Type::Unknown)]
            ),
            Some(Type::string())
        );
        // Yaml + non-Yaml method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Compile, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Debug, &[]),
            None
        );
        // Non-Yaml type + Yaml method is invalid (the (Type, Parse)
        // pair is validated by the registry matrix; Yaml lowers via
        // (Yaml, Parse), Toml via (Toml, Parse), but (Regex, Stringify)
        // is invalid - Stringify is shared by Toml/Yaml/Csv ONLY).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Stringify, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Stringify, &[]),
            None
        );
    }

    // T124i: Csv module - `Csv.parse(s)` / `Csv.stringify(rows)` assoc-fn
    // lookups + return types. Mirrors the Yaml / Toml namespace-only
    // precedent but with `Vector<Vector<String>>` instead of Map.
    #[test]
    fn prelude_csv_assoc_fn_lookup_valid_pairs() {
        // `Csv.parse` reuses the registry's shared `Parse` variant.
        assert_eq!(
            assoc_fn_lookup("Csv", "parse"),
            Some((PreludeType::Csv, PreludeAssocFn::Parse))
        );
        // `Csv.stringify` reuses the registry's shared `Stringify` variant.
        assert_eq!(
            assoc_fn_lookup("Csv", "stringify"),
            Some((PreludeType::Csv, PreludeAssocFn::Stringify))
        );
        // `Csv` is recognised as a prelude type.
        assert!(is_prelude_type("Csv"));
        // `Csv.buff_type()` is `Void` (no runtime value - namespace-only).
        assert_eq!(PreludeType::Csv.buff_type(), Type::Void);
        // `Csv.is_namespace_only()` is true.
        assert!(PreludeType::Csv.is_namespace_only());
    }

    #[test]
    fn prelude_csv_assoc_fn_lookup_rejects_invalid_pairs() {
        // Csv.now is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "now"), None);
        // Csv.compile is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "compile"), None);
        // Csv.debug is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "debug"), None);
        // Csv.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "unknown"), None);
        // Csv.split is invalid (split belongs to Strings).
        assert_eq!(assoc_fn_lookup("Csv", "split"), None);
        // Yaml now/compile invalid (mirror Yaml rejects).
        assert_eq!(assoc_fn_lookup("Yaml", "now"), None);
    }

    #[test]
    fn prelude_csv_assoc_fn_return_types() {
        // Csv.parse(s) -> Vector<Vector<String>>. Uniform rows (NO
        // header special-casing per the spec - every row including
        // the header is a Vector<String>). Cells are String (CSV has
        // no inherent type information, every cell is text).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Parse, &[Type::string()]),
            Some(Type::vector(Type::vector(Type::string())))
        );
        // Csv.stringify(rows) -> String. The arg is a
        // Vector<Vector<String>>; the codegen builds a csv::Writer
        // and serializes.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Csv,
                PreludeAssocFn::Stringify,
                &[Type::vector(Type::vector(Type::string()))]
            ),
            Some(Type::string())
        );
        // Csv + non-Csv method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Compile, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Debug, &[]),
            None
        );
        // Non-Csv type + Csv method: a hypothetical `(Toml, Parse)`
        // pair is valid (Toml reuses Parse), but (Regex, Stringify)
        // is invalid (Stringify is shared by Toml/Yaml/Csv only).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Stringify, &[]),
            None
        );
    }

    // T124j: Path module - `Path.join(a, b, ...)` assoc-fn lookup +
    // return type + the four instance methods. Mirrors the URL
    // runtime-value-with-methods precedent (T124h) - Path is the
    // third such type (after Regex T124d + URL T124h).
    #[test]
    fn prelude_path_assoc_fn_lookup_valid_pairs() {
        // `Path.join` reuses the registry's shared `Join` variant
        // (also used by Strings.join from T124f). Same name,
        // different per-type semantics dispatched on the (Path,
        // Join) pair.
        assert_eq!(
            assoc_fn_lookup("Path", "join"),
            Some((PreludeType::Path, PreludeAssocFn::Join))
        );
        // `Path` is recognised as a prelude type.
        assert!(is_prelude_type("Path"));
        // `Path.buff_type()` is `Path` (a real runtime value, NOT
        // Void - mirrors Regex / URL).
        assert_eq!(PreludeType::Path.buff_type(), Type::Path);
        // `Path.is_namespace_only()` is false (it IS a runtime value).
        assert!(!PreludeType::Path.is_namespace_only());
        // The other prelude types are NOT Path (round-trip via
        // buff_type).
        assert!(!PreludeType::DateTime.buff_type().is_prelude_path());
        assert!(!PreludeType::Regex.buff_type().is_prelude_path());
        assert!(!PreludeType::URL.buff_type().is_prelude_path());
        assert!(PreludeType::Path.buff_type().is_prelude_path());
    }

    #[test]
    fn prelude_path_assoc_fn_lookup_rejects_invalid_pairs() {
        // Path.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Path", "now"), None);
        // Path.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Path", "compile"), None);
        // Path.parse is invalid (Path has join, not parse).
        assert_eq!(assoc_fn_lookup("Path", "parse"), None);
        // Path.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Path", "unknown"), None);
        // Path.list is invalid (list belongs to Args/Dir).
        assert_eq!(assoc_fn_lookup("Path", "list"), None);
        // DateTime.join is invalid (join belongs to Strings/Path).
        assert_eq!(assoc_fn_lookup("DateTime", "join"), None);
        // Strings.join IS valid (reuses Join) - confirms Path.join
        // is also valid via the same overload-by-type pattern.
        assert_eq!(
            assoc_fn_lookup("Strings", "join"),
            Some((PreludeType::Strings, PreludeAssocFn::Join))
        );
    }

    #[test]
    fn prelude_path_assoc_fn_return_type() {
        // Path.join(a, b, ...) -> Path.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Path,
                PreludeAssocFn::Join,
                &[Type::string(), Type::string()]
            ),
            Some(Type::Path)
        );
        // Single-arg join is also valid (returns PathBuf of the arg).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Path, PreludeAssocFn::Join, &[Type::string()]),
            Some(Type::Path)
        );
        // Path + non-Path method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Path, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Path, PreludeAssocFn::Compile, &[]),
            None
        );
        // Non-Path type + Path method: a hypothetical `(Strings, Join)`
        // pair is valid (Strings reuses Join), but (Log, Join) is
        // invalid (Log is namespace-only with debug/info/...).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Join, &[]),
            None
        );
    }

    #[test]
    fn prelude_path_instance_fn_lookup_valid_pairs() {
        // All four Path instance methods resolve via the registry
        // when the receiver is `Type::Path`.
        assert_eq!(
            instance_fn_lookup(&Type::Path, "parent"),
            Some(PreludeInstanceFn::Parent)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Path, "extension"),
            Some(PreludeInstanceFn::Extension)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Path, "basename"),
            Some(PreludeInstanceFn::Basename)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Path, "exists"),
            Some(PreludeInstanceFn::Exists)
        );
    }

    #[test]
    fn prelude_path_instance_fn_lookup_rejects_invalid_pairs() {
        // Path.format is invalid (format belongs to DateTime/Date/Time).
        assert_eq!(instance_fn_lookup(&Type::Path, "format"), None);
        // Path.year is invalid.
        assert_eq!(instance_fn_lookup(&Type::Path, "year"), None);
        // Path.unknown is invalid.
        assert_eq!(instance_fn_lookup(&Type::Path, "unknown"), None);
        // Path.parent is invalid when the receiver is NOT Path.
        assert_eq!(instance_fn_lookup(&Type::DateTime, "parent"), None);
        assert_eq!(instance_fn_lookup(&Type::String, "exists"), None);
    }

    #[test]
    fn prelude_path_instance_fn_return_types() {
        // path.parent() -> Option<Path>.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Parent, &[]),
            Some(Type::option(Type::Path))
        );
        // path.extension() -> Option<String>.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Extension, &[]),
            Some(Type::option(Type::string()))
        );
        // path.basename() -> String.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Basename, &[]),
            Some(Type::string())
        );
        // path.exists() -> Bool.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Exists, &[]),
            Some(Type::bool())
        );
        // Non-Path receiver + Path method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Parent, &[]),
            None
        );
        // Path receiver + non-Path method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Format, &[Type::string()]),
            None
        );
    }

    // T124j: Dir module - `Dir.list/create/remove/walk` assoc-fn
    // lookups + return types. Mirrors the Yaml / Csv namespace-only
    // precedent (T124i) but for filesystem operations.
    #[test]
    fn prelude_dir_assoc_fn_lookup_valid_pairs() {
        // `Dir.list` reuses the registry's shared `List` variant
        // (also used by Args.list from T124g).
        assert_eq!(
            assoc_fn_lookup("Dir", "list"),
            Some((PreludeType::Dir, PreludeAssocFn::List))
        );
        // `Dir.create` is the new shared Create variant (shared
        // with Tempfile.create).
        assert_eq!(
            assoc_fn_lookup("Dir", "create"),
            Some((PreludeType::Dir, PreludeAssocFn::Create))
        );
        // `Dir.remove` is the new Dir-only Remove variant.
        assert_eq!(
            assoc_fn_lookup("Dir", "remove"),
            Some((PreludeType::Dir, PreludeAssocFn::Remove))
        );
        // `Dir.walk` is the new Dir-only Walk variant.
        assert_eq!(
            assoc_fn_lookup("Dir", "walk"),
            Some((PreludeType::Dir, PreludeAssocFn::Walk))
        );
        // `Dir` is recognised as a prelude type.
        assert!(is_prelude_type("Dir"));
        // `Dir.buff_type()` is `Void` (no runtime value - namespace-only
        // like Log / Toml / Yaml / Csv).
        assert_eq!(PreludeType::Dir.buff_type(), Type::Void);
        // `Dir.is_namespace_only()` is true.
        assert!(PreludeType::Dir.is_namespace_only());
    }

    #[test]
    fn prelude_dir_assoc_fn_lookup_rejects_invalid_pairs() {
        // Dir.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Dir", "now"), None);
        // Dir.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Dir", "compile"), None);
        // Dir.parse is invalid (Dir has list/create/remove/walk, not parse).
        assert_eq!(assoc_fn_lookup("Dir", "parse"), None);
        // Dir.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Dir", "unknown"), None);
        // Dir.join is invalid (join belongs to Strings/Path).
        assert_eq!(assoc_fn_lookup("Dir", "join"), None);
        // Dir.encode is invalid (encode belongs to Base64/Hex/URLEncode).
        assert_eq!(assoc_fn_lookup("Dir", "encode"), None);
        // DateTime.walk is invalid (walk belongs to Dir).
        assert_eq!(assoc_fn_lookup("DateTime", "walk"), None);
        // Regex.remove is invalid (remove belongs to Dir).
        assert_eq!(assoc_fn_lookup("Regex", "remove"), None);
    }

    #[test]
    fn prelude_dir_assoc_fn_return_types() {
        // Dir.list(path) -> Vector<String>.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::List, &[Type::Path]),
            Some(Type::vector(Type::string()))
        );
        // Dir.create(path) -> Void.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Create, &[Type::Path]),
            Some(Type::Void)
        );
        // Dir.remove(path) -> Void.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Remove, &[Type::Path]),
            Some(Type::Void)
        );
        // Dir.walk(path) -> Vector<Path>.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Walk, &[Type::Path]),
            Some(Type::vector(Type::Path))
        );
        // Dir + non-Dir method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Compile, &[]),
            None
        );
        // Non-Dir type + Dir method is invalid (the (Type, List) pair
        // is shared by Args/Dir but (DateTime, Walk) is invalid).
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Walk, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Remove, &[]),
            None
        );
    }

    // T124j: Tempfile module - `Tempfile.create/dir` assoc-fn
    // lookups + return types. Mirrors the Dir namespace-only precedent.
    #[test]
    fn prelude_tempfile_assoc_fn_lookup_valid_pairs() {
        // `Tempfile.create` reuses the new shared Create variant
        // (also used by Dir.create).
        assert_eq!(
            assoc_fn_lookup("Tempfile", "create"),
            Some((PreludeType::Tempfile, PreludeAssocFn::Create))
        );
        // `Tempfile.dir` is the new Tempfile-only Dir variant.
        assert_eq!(
            assoc_fn_lookup("Tempfile", "dir"),
            Some((PreludeType::Tempfile, PreludeAssocFn::Dir))
        );
        // `Tempfile` is recognised as a prelude type.
        assert!(is_prelude_type("Tempfile"));
        // `Tempfile.buff_type()` is `Void` (no runtime value -
        // namespace-only like Dir / Log / Toml / Yaml / Csv).
        assert_eq!(PreludeType::Tempfile.buff_type(), Type::Void);
        // `Tempfile.is_namespace_only()` is true.
        assert!(PreludeType::Tempfile.is_namespace_only());
    }

    #[test]
    fn prelude_tempfile_assoc_fn_lookup_rejects_invalid_pairs() {
        // Tempfile.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Tempfile", "now"), None);
        // Tempfile.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Tempfile", "compile"), None);
        // Tempfile.parse is invalid (Tempfile has create/dir, not parse).
        assert_eq!(assoc_fn_lookup("Tempfile", "parse"), None);
        // Tempfile.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Tempfile", "unknown"), None);
        // Tempfile.list is invalid (list belongs to Args/Dir).
        assert_eq!(assoc_fn_lookup("Tempfile", "list"), None);
        // Tempfile.walk is invalid (walk belongs to Dir).
        assert_eq!(assoc_fn_lookup("Tempfile", "walk"), None);
        // DateTime.dir is invalid (dir belongs to Tempfile).
        assert_eq!(assoc_fn_lookup("DateTime", "dir"), None);
        // Dir.dir is invalid (Dir has list/create/remove/walk, not dir).
        assert_eq!(assoc_fn_lookup("Dir", "dir"), None);
    }

    #[test]
    fn prelude_tempfile_assoc_fn_return_types() {
        // Tempfile.create() -> Path.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Create, &[]),
            Some(Type::Path)
        );
        // Tempfile.dir() -> Path.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Dir, &[]),
            Some(Type::Path)
        );
        // Tempfile + non-Tempfile method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Compile, &[]),
            None
        );
        // Non-Tempfile type + Tempfile method: (Dir, Create) is valid
        // (Dir reuses Create), but (Regex, Dir) is invalid (Dir method
        // is Tempfile-only).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Dir, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Dir, &[]),
            None
        );
    }
