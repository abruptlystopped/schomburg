# Connectors

## Git

Git is the only production connector. It discovers repositories under explicit scan roots, to a bounded depth, skips `.git`, `.schomburg`, build, distribution, `target`, and `node_modules` directories, and does not follow symlinks. It imports commits reachable from current `HEAD` only after consent.

Event IDs derive from canonical Git-directory identity and commit hash; repeats are duplicates, not overwrites. Moving a repository changes identity. Compact and detailed views are factual; `event --raw` retains raw commit evidence.

## Not implemented

VS Code, Terminal, Research/browser, Claude, ChatGPT, Calendar, Email, and Documents.
