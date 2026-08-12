# Consumed Argument Error Preservation

An operation returning `Result<(), E>` exposes a failure channel whenever `E` is inhabited.
Rivus therefore requires every owned, non-copy input type to be represented by `E`. The rule is
based on the signature and does not attempt to prove which branch the current implementation takes.

This is deliberately conservative. A function that currently returns only `Ok(())` still has an
inhabited error channel and may gain an error path without changing its signature. Preserving the
input in the error type keeps that future change safe.

References and copyable inputs do not transfer ownership and are excluded. An uninhabited error
type cannot represent failure and is also excluded. No control-flow, alias, callback, coroutine,
drop, pointer, or projection analysis is part of this rule.
