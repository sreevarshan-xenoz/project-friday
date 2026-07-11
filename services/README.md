# services/

Long-lived, addressable capabilities hosted by the runtime. A service exposes a
stable identity and lifecycle (`start`/`stop`) and is reachable over the protocol
transport. Typically a plugin hosts one or more services.

Status: scaffolding only. The `Service` trait is defined in
`core/src/services/traits.rs`.
