# codepal

Codepal MCP server - Coding Pal

## MCP Instructions / System Prompts

This MCP server includes a set of instructions to tune agents for code-related tasks.
These instructions are injected as system prompt context when agents connect.

The active instruction set depends on which options are enabled:

- **Base instructions**: Always included.
- **Compressed comm instructions**: Included when `--enable-compressed` is active.
- **Memory instructions**: Included when `--enable-memory` is active.
- **Rust instructions**: Included automatically when a `Cargo.toml` is detected in the workspace.

### Compressed AI communication mode

When enabled with `--enable-compressed`, the server instructs agents to use a compressed communication format: filler words and redundant information are removed, keeping only essential data.
This reduces token usage.

## MCP Prompts

`doit`

- Description: Execute arbitrary instructions with CodePal context.
- Parameters: `instructions`: Instructions to execute.

`security_audit`

- Description: Perform a security audit guided by CodePal.
- Parameters: `what`: Artifact to audit.

`find_bugs`

- Description: Find bugs in the specified code or artifact.
- Parameters: `what`: What to find bugs in.

`find_performance_improvements`

- Description: Find performance improvements in the specified code or artifact.
- Parameters: `what`: What to find performance improvements in.

`refactor`

- Description: Refactor the specified code or artifact.
- Parameters: `what`: What to refactor.

## MCP Tools

`ls(path: string)`

- List directory contents under *Allowed Paths*.
- Parameters: `path`: Directory path to list.
- Returns: directory listing.

`read(path: string, start_line?: number, end_line?: number)`

- Read file contents under *Allowed Paths*.
- Parameters:
  - `path`: File to read.
  - `start_line` (optional): First line to read, 1-based inclusive.
  - `end_line` (optional): Last line to read, 1-based inclusive.
- Returns: File text (up to 256 KB).

`grep(path: string, pattern: string, case_insensitive?: bool, dot_matches_newline?: bool, context_lines?: number)`

- Search file or directory contents under *Allowed Paths* using a regular expression.
- Parameters:
  - `path`: File or directory path to search.
  - `pattern`: Regex pattern to match.
  - `case_insensitive` (optional): Enable case-insensitive matching (default: `false`).
  - `dot_matches_newline` (optional): Make `.` match `\n` (default: `false`).
  - `context_lines` (optional): Number of context lines before/after each match (like `grep -C`, default: `0`).
- Returns: Lines containing matches plus optional context. Output format:
  - Matching lines are prefixed with `N:line` (line number + `:`).
  - Context lines are prefixed with `N-line` (line number + `-`).
  - Separate match groups are delimited by `--` on its own line.
  - Empty string returned when no matches found.

`find(path: string, pattern: string, case_insensitive?: bool)`

- Find files matching a regex pattern in a directory tree under *Allowed Paths*.
- Parameters:
  - `path`: Directory to search recursively.
  - `pattern`: Regex matched against relative file paths.
  - `case_insensitive` (optional): Enable case-insensitive matching (default: `true`).
- Returns: List of matching absolute file paths (up to 10,000).

`mem_list()`

- List all keys in the persistent memory store.
- Returns: List of keys.
- Only available when `--enable-memory` is active.

`mem_store(keys: string[], value: string)`

- Store a value in the key-value memory store. All provided keys map to the same value.
- Parameters:
  - `keys`: one or more keys to associate with the value (max 64 keys, max 256 bytes each).
  - `value`: value to store (max 64 KB).
- Only available when `--enable-memory` is active.

`mem_load(keys: string[])`

- Load a value from the memory store. Returns the value for the first key found.
- Parameters:
  - `keys`: keys to look up; the first match wins. Supports fuzzy/partial key matching when no exact match exists.
- Returns: stored value, or `null` if not found.
- Only available when `--enable-memory` is active.

`mem_delete(keys: string[])`

- Delete one or more keys from the memory store.
- Parameters:
  - `keys`: keys to delete.
- Returns: list of keys that were actually deleted.
- Only available when `--enable-memory` is active.

### Allowed paths

- Only files under *Allowed Paths* may be read or listed.
- The workspace root is always included in *Allowed Paths*.
- Additional paths can be added with `--allow-read`.
- For Rust projects, `~/.cargo` and `~/.rustup` are added automatically (unless `--no-auto-path-allow` is set).

## Command line options

See `codepal --help` for the full list of options.

### Memory storage

When `--enable-memory` is active, the memory store is persisted in a SQLite database named `.agents-codepal-memory.sqlite` in the workspace root.

Use `--dump-memory` to inspect the stored keys and values from the command line without starting the server.

Use `--memory-max-age-days` to automatically prune entries that have not been accessed within the given number of days. Pruning runs on each memory tool call.

## VS Code MCP client configuration

`.vscode/mcp.json` example configuration for VS Code MCP client:

```json
{
  "servers": {
    "codepal": {
      "type": "stdio",
      "command": "/opt/codepal/bin/codepal",
      "args": [
         "--workspace=${workspaceFolder}"
        ,"--enable-compressed"
        ,"--enable-memory"
      ],
      "env": {}
    }
  }
}
```

## Claude Code configuration

Create `.mcp.json` in the project root with the following content:

```json
{
  "mcpServers": {
    "codepal": {
      "command": "/opt/codepal/bin/codepal",
      "args": [
         "--workspace=."
        ,"--enable-compressed"
        ,"--enable-memory"
      ],
      "env": {}
    }
  }
}
```

## Build and install

```sh
./build.sh    # compile release binary
./install.sh  # install to /opt/codepal/bin/codepal
```

## Acknowledgements

Skill instructions derived from [caveman](https://github.com/juliusbrussee/caveman)

## License

MIT License. See [LICENSE](LICENSE) for details.
