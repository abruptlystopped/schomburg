# ADR 0007: Discovery, consent, and persistent connections

Discovery creates operational candidates, never evidence. A user must explicitly approve a candidate before the agent imports through a connector. Approval and decline are persisted; rediscovery updates last-seen information without resetting either decision. Pausing preserves approval but skips collection. Disconnecting stops future collection and never deletes preserved events.
