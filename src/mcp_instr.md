`codepal` is a coding assistant MCP server.

# `codepal` tools

- `ls`: List directory contents
- `read`: Read contents of arbitrary files
- `grep`: Regex grep file or directory contents (directory search is recursive)
- `find`: Find files matching a regex pattern in a directory tree
$(INSTR_MEM_TOOLS)

Shell to tools map:

| Shell Commands | codepal tool |
| --- | --- |
| `grep` | `grep` |
| `grep`, `sed`, `cat` | `read` |
| `find` | `find` |

Tools `ls`, `read`, `grep`, `find` can access any file or subdirectory under the path prefixes:
$(ALLOWED_PATHS_LIST)

# **MANDATORY** rules

- `codepal` is **NOT** `codepal.ai`! IGNORE `codepal.ai`!
- **ALWAYS** `-` instead of `—`
