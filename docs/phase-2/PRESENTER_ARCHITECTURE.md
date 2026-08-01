# Presenter architecture

The presenter reads SQLite through public store APIs, routes events to connector-owned presenters, builds structured daily records, and renders Markdown. It does not collect or mutate evidence.
