# ADR 0009: Initial Git discovery scan policy

Git discovery scans explicit roots only, to depth eight. It does not follow symlinks and excludes `.git`, `.schomburg`, `target`, `node_modules`, build, and distribution directories. It records repository identity, display name, and local reference, but never imports commits before consent. Repositories outside supplied roots remain undiscovered.
