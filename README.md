# codepal

Codepal MCP server - Coding Pal

## MCP Instructions / System Prompts

This MCP server includes a set of instructions to tune agents for code-related tasks.
These instructions are provided as system prompts to agents and define the tools and operations available to them.

### Compressed AI communication mode

- When enabled with the `--enable-compressed` option, the server uses a compressed communication format with agents.
- Filler words and redundant information are removed from tool responses, and only essential data is kept.
- This mode reduces the token usage.

## MCP Tools

- `ls_dir(path: string)`
  - List directory contents under *Allowed Paths*.
  - Parameters: `path` = directory path to list (workspace-relative or absolute depending on agent context).
  - Returns: directory listing (primary mandatory directory-list tool for MCP operations).

- `read_file(path: string, start_line?: number, end_line?: number)`
  - Read file contents under *Allowed Paths*.
  - Parameters: `path`, optional `start_line` / `end_line`.
  - Returns: file text chunk or full content.

- `grep_file(path: string, pattern: string, case_insensitive?: bool, dot_matches_newline?: bool, context_lines?: number)`
  - Search file contents under *Allowed Paths* using a regular expression.
  - Parameters:
    - `path`: file path to search (must be under *Allowed Paths*).
    - `pattern`: regex pattern to match.
    - `case_insensitive` (optional): enable case-insensitive matching (default: `false`).
    - `dot_matches_newline` (optional): make `.` match `\n` (default: `false`).
    - `context_lines` (optional): number of context lines before/after each match (like `grep -C`, default: `0`).
  - Returns: lines containing matches plus optional context. Output format:
    - Matching lines are prefixed with `N:line` (line number + `:`).
    - Context lines are prefixed with `N-line` (line number + `-`).
    - Separate match groups are delimited by `--` on its own line.
    - Empty string returned when no matches found.

### Allowed paths

- Only files under *Allowed Paths* may be read or listed.
- *Allowed Paths* are specified by the user when starting the server.
- The project workspace root is always included in *Allowed Paths*.

## VS Code MCP client configuration

`.vscode/mcp.json` example configuration for VS Code MCP client:

```json
{
  "servers": {
    "codepal": {
      "type": "stdio",
      "command": "/opt/codepal/bin/codepal",
      "args": [
        "--workspace", "${workspaceFolder}",
        "--allow-read", "${env:HOME}/.cargo",
        "--allow-read", "${env:HOME}/.rustup",
        "--enable-compressed",
      ],
      "env": {}
    }
  }
}
```

## Acknowledgements

Skill instructions derived from [caveman](https://github.com/juliusbrussee/caveman)

## License

MIT License. See [LICENSE](LICENSE) for details.
