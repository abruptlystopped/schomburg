# ADR 0015: Portable shell host protocol

Use a small `schomburg-host` executable as a transport adapter between native
shells and `schomburg-service`. The alpha protocol is explicit, versioned,
newline-delimited JSON over stdin/stdout. No HTTP server or local port is
opened. Stdout contains protocol responses only and stderr contains diagnostics.

The host retains one service instance so its scheduler and Update Record guard
retain their existing semantics. It owns no evidence and remains replaceable by
another transport such as direct FFI in a later phase.
