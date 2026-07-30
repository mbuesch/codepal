# Your AI assistant long term memory

You → No long term memory. Next session you forget *everything*.
`mem_store` → **your long term memory**.
Use it, else you only have short term memory.
Long term memory → remember between sessions.

More likely to `mem_store` if:
- Worked hard to get information.
- Information not obvious from code/docs.

**MANDATORY** rules:

- **ALWAYS** query `mem_list` and `mem_load` tools for relevant information before analyzing.
- Store *useful* new insights about the project that are *worth remembering* with the `mem_store` tool.
- **MUST** use short keys, *descriptive of the content*, but *not too specific*.
- Stored values **MUST** be *short*, *brief* and to the point.
- **ALWAYS** update memory, if *code* or *documentation* is changed, to keep it up-to-date, using `mem_store` tool.
- DELETE memory keys that are: Wrong, irrelevant, outdated, duplicate. Using `mem_delete` tool. You decide what/when to delete. No restriction.

Good example entries:

| Key | Value |
| --- | --- |
| `overview` | `Databuzzler; Always encrypted; Quantum hardened` |
| `frobulator` | `Tool; Frobulate data; Impl tools/frob.rs` |
| `wire-format` | `Net format; Doc doc/wire/; Impl wire_format/` |
| `forbidden` | `No plain text` |
| `good-practices` | `Organize into mods; Update docs; Use hw acceleration` |
| `adc` | `ADC=1023 → full brightness; Dead band = 8; <32 → non-linear` |
| `todo` | `Add frobulator; Fix buzzler` |
