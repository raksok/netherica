
### Phase 12 ↔ Phase 13 Dependency Map

> **Execution order:** Phase 13 tasks should be implemented *alongside* or *before* their corresponding Phase 12 UI tasks.
> Tasks 12.1–12.8 (module split, primitives, sidebar) have **no** Phase 13 dependencies and can proceed immediately.

| Phase 12 (UI) | Depends on Phase 13 (State/Logic) | Reason |
|---|---|---|
| 12.2 (NavigationSection) | 13.1 (Enums + state fields) | Enum definition and state wiring |
| 12.9 (Idle view) | 13.2 (Last-run query), 13.3 (DB status) | Status cards need startup data |
| 12.10 (Parsing view) | 13.5 (Structured messages), 13.6 (Log state), 13.14 (Worker refactor), 13.15 (Handler update) | File metadata card and live log depend on structured worker messages |
| 12.11 (Dry Run view) | 13.7 (Computation time), 13.8 (Warning count), 13.9 (Accuracy metric) | Metric cards need computed domain data |
| 12.12 (Completion view) | 13.4 (APP_VERSION), 13.10 (Pipeline time), 13.11 (Outcome metrics), 13.12 (Data integrity) | Metadata footer needs all tracked metrics |
| 12.13 (Settings routing) | 13.1 (SettingsTab enum) | Tab state management |
| 12.14 (Departments) | — | Uses existing `config.departments`, no new logic |
| 12.15 (Products) | — | Uses existing `config.products`, no new logic |
