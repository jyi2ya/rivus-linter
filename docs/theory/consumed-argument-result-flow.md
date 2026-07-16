# Consumed Argument Result Flow

An owned input is at risk only when an operation can finish by reporting failure. If every
completion reports success, consuming the input does not require preserving it in an error.

A reported failure may be carried directly, stored inside another value, selected after a
branch, or produced by another operation. Observing a success-or-failure value does not change
which outcome it contains. Mutating data carried by a success outcome also does not change the
outcome itself.

Aliases distinguish the outcome they refer to from outcomes reachable inside its payload. An
observer may be unable to replace the outer outcome while still retaining authority to replace
a nested outcome. This distinction must survive calls and joins.

Different fields normally describe different storage. Fields of a union describe the same
storage and therefore invalidate one another when written.

When an operation has no inspectable implementation, its effect is unknown. Missing knowledge
must not be treated as success, and attempting to inspect a nonexistent implementation must not
abort analysis.
