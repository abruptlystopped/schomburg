# ADR 0010: Daily Record Folder

The presenter builds structured source-agnostic daily read models and renders generated Markdown under numeric year/month folders. SQLite remains authoritative; files are disposable generated readouts, atomically replaced when changed. Local time at generation determines date grouping.
