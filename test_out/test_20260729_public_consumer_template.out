# Rivus Project Rules

This section contains the public rules enforced or supported by `cargo rivus`.

## Function contracts

- Project functions and trait methods use the `rvs_` prefix. A final all-uppercase suffix records suffix capabilities in `BIMPST` order. A/C/U are measured from the signature and body facts and must not appear in the suffix.
- Primitive numeric parameters on `rvs_` functions require an appropriate `debug_assert!`, `debug_assert_eq!`, or `debug_assert_ne!` contract.
- Public `rvs_` functions require `///` documentation. Unsafe functions also require a `# Safety` section.

## Capabilities

| Capability | Meaning |
|------------|---------|
| `A` | The function is `async fn` (never in the suffix) |
| `B` | The function may block the current thread |
| `C` | The function is `const fn` (never in the suffix) |
| `I` | The function performs I/O |
| `M` | The function accepts mutable state through `&mut` |
| `P` | The function depends on a local Port trait |
| `S` | The function observes or changes ambient/global state |
| `T` | The function depends on thread-local state |
| `U` | The function is `unsafe fn` or accesses `static mut` (never in the suffix) |

- The propagated barriers are exactly `B/I/P/S/T`. Ordinary calls require every propagated callee capability; if the callee contains `P`, that call edge requires only `P`.
- `A/C/U` are signature/body capabilities measured directly from the function. They do not propagate through calls and never appear in name suffixes; reports and statistics still count them.
- A local trait is a World Port when it declares one non-generic associated type named `World`, has at least one static operation, and every operation explicitly accepts `&Self::World` or `&mut Self::World`. Its name is irrelevant. Additional associated types represent long-lived resources; associated constants, receiver methods, generic World types, and operations without a World reference make it an ordinary trait.
- Every World Port operation has a public contract of exactly `P`: both the trait method and each implementation are named with the `_P` suffix. Implementation effects (`B/I/S/T`) are audit information surfaced through `cargo rivus report` and `cargo rivus why`; they never enter the domain calling contract, and `A/C/M/U` are still measured from the signature and body. A call to any `P`-containing contract propagates only `P` upward, and implementation knowledge gaps do not leak past the Port boundary. Use a type-level interpreter and caller-owned World instead of `Box<dyn>` service objects.
- `Result/Option` handling is type-system error flow, not a capability. Returning or propagating either type adds no capability letter.

## Errors and tests

- Fallible code uses domain-specific `Result<T, E>` errors. Use Snafu; Rivus rejects `thiserror`, `anyhow`, `eyre`, and `color_eyre` imports.
- Do not discard `Result` errors with `.ok()`, `.unwrap_or_default()`, or `drop`.
- Tests use unique names in the form `test_YYYYMMDD_name`.
- When the project has a `test_out/` directory, each test has a matching `test_out/{test_name}.out` snapshot.
- Prefer structured concurrency such as `join!`, `JoinSet`, `FuturesUnordered`, or `thread::scope` over unscoped spawn calls.

## Commands

```bash
cargo rivus check
cargo rivus report .
cargo rivus annotate .
cargo rivus strip .
cargo rivus why path::to::function .
cargo rivus infer-std -o caps/std
cargo rivus infer-capsmap -o caps/deps
cargo rivus usage
```

Caps layers use capsmap v2 JSON Lines under the project-root `caps/` directory. `infer-std` and `infer-capsmap` only write their explicitly supplied `-o` paths.
