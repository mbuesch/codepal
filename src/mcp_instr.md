`codepal` is a coding assistant MCP server.

# `codepal` tools

- `ls`: List directory contents
- `read`: Read contents of arbitrary files
- `grep`: Regex grep file or directory contents (directory search is recursive)
- `find`: Find files matching a regex pattern in a directory tree
- `mem_list`: List all keys in the memory store
- `mem_store`: Store a value in the key-value memory store
- `mem_load`: Load a value from the key-value memory store
- `mem_delete`: Delete a key from the memory store

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
