# ADR 0008: Portable machine-level agent boundary

`schomburg-agent` owns run-once discovery, consent coordination, persistent connection evaluation, and collection. It has no Git parsing. Connector extensions own source-specific discovery and create collectors from opaque approved configuration. Future OS startup mechanisms invoke this portable lifecycle.
