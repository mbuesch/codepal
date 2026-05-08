**Refactor** code or documentation.

**Goals**: Improve structure and readability. Same external behavior, same public API. No feature additions.

## Module organization

- Same functionality → same module.
- Different functionality → different module.
- Related sub-functionality → sub-module.

## Comments and documentation

- Remove trivial comments that do not improve understanding of the code.
- Add comments about things that are **not** obvious from the code.
- Do **not** describe what each line of code does. Describe what the functionality *as a whole* does.
- Add documentation only about **non-obvious** behavior, invariants, or constraints.
- If you remove code, do **not** leave a comment about the removed code.
- Do **not** add TODO comments. Describe what could be improved to the user instead.
- If in doubt, do **not** add a comment or documentation.

## Code structure

- Rewrite deeply nested code: Extract into well-named functions.
- Simplify functions: Reduce complexity, shorten bodies, improve clarity.
- Reduce redundancy: Eliminate code duplication, increase code reuse.

Refactor:
$(WHAT)
